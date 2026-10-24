//! Shared fixtures for unit tests: a scripted fake upstream over raw TCP,
//! the rustls provider install every HTTP client test needs, and `.gem`
//! files built in memory.

#[cfg(feature = "gems")]
use std::path::Path;
use std::{
    io::{Cursor, Write},
    sync::{Arc, Mutex, Once},
};

use flate2::{Compression, write::GzEncoder};
#[cfg(feature = "gems")]
use sha2::{Digest, Sha256};
use tar::{Builder, Header};
#[cfg(feature = "gems")]
use tempfile::NamedTempFile;

use rama::tls::rustls::dep::rustls;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

/// Installs the aws-lc-rs crypto provider once per process; rama's TLS client
/// panics without one.
pub fn install_rustls_provider() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // A second install (another provider already set) is not an error here.
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

/// A raw HTTP/1.1 response with `Content-Length` and `Connection: close`.
pub fn raw_response(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    use std::fmt::Write as _;

    let mut response = format!("HTTP/1.1 {status}\r\n");
    for (name, value) in headers {
        write!(response, "{name}: {value}\r\n").expect("writing to a String never fails");
    }
    write!(response, "Content-Length: {}\r\n", body.len())
        .expect("writing to a String never fails");
    response.push_str("Connection: close\r\n\r\n");

    let mut bytes = response.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

/// Serves `responses` in order, one per connection, on a random loopback port.
/// Returns the base URL and a handle resolving to the lower-cased request heads.
#[cfg(any(feature = "crates", feature = "npm"))]
pub async fn spawn_sequence_server(
    responses: Vec<Vec<u8>>,
) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        let mut requests = Vec::new();
        for response in responses {
            let (mut socket, _) = listener.accept().await.unwrap();
            requests.push(read_http_request(&mut socket).await);
            socket.write_all(&response).await.unwrap();
        }
        requests
    });

    (format!("http://{addr}"), handle)
}

/// `200 OK` with `body` as plain text: the usual route answer.
pub fn ok(body: impl Into<Vec<u8>>) -> Vec<u8> {
    raw_response("200 OK", &[("Content-Type", "text/plain")], &body.into())
}

/// Answers `GET {path}` (request target, query included) with its raw
/// response ([`ok`], or [`raw_response`] for headers), anything else with
/// `404`, for as long as the test runs, on a random loopback port. Returns
/// the port and the lower-cased request heads received so far.
pub async fn spawn_route_server(routes: Vec<(String, Vec<u8>)>) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));

    let log = Arc::clone(&seen);
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let request = read_http_request(&mut socket).await;
            let path = request.split_whitespace().nth(1).unwrap_or_default();
            let response = routes
                .iter()
                .find(|(route, _)| route.eq_ignore_ascii_case(path))
                .map_or_else(
                    || raw_response("404 Not Found", &[], b"not found"),
                    |(_, response)| response.clone(),
                );
            log.lock().unwrap().push(request);
            socket.write_all(&response).await.unwrap();
        }
    });

    (port, seen)
}

/// Reads up to the end of the request head and lower-cases it for matching.
async fn read_http_request(socket: &mut TcpStream) -> String {
    let mut buffer = vec![0_u8; 4096];
    let mut request = Vec::new();

    loop {
        let read = socket.read(&mut buffer).await.unwrap();
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }

    String::from_utf8_lossy(&request).to_ascii_lowercase()
}

/// A `.gem` file: `metadata.gz` with `metadata_yaml`, and a `data.tar.gz`
/// holding `data_entries` (path, contents).
#[cfg(feature = "gems")]
pub fn build_test_gem(metadata_yaml: &str, data_entries: &[(&str, &[u8])]) -> NamedTempFile {
    let mut metadata_encoder = GzEncoder::new(Vec::new(), Compression::default());
    metadata_encoder
        .write_all(metadata_yaml.as_bytes())
        .expect("write metadata yaml");
    let metadata_bytes = metadata_encoder
        .finish()
        .expect("finalize metadata gzip payload");

    let mut data_tar_bytes = Vec::new();
    {
        let mut data_builder = Builder::new(&mut data_tar_bytes);
        for (path, contents) in data_entries {
            let mut header = Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            data_builder
                .append_data(&mut header, path, Cursor::new(*contents))
                .expect("append data file");
        }
        data_builder.finish().expect("finish data tar");
    }

    let mut data_encoder = GzEncoder::new(Vec::new(), Compression::default());
    data_encoder
        .write_all(&data_tar_bytes)
        .expect("compress data tar");
    let data_bytes = data_encoder.finish().expect("finalize data gzip payload");

    let file = NamedTempFile::new().expect("create temp gem");
    {
        let mut handle = file.reopen().expect("reopen temp gem");
        {
            let mut builder = Builder::new(&mut handle);

            let mut header = Header::new_gnu();
            header.set_size(metadata_bytes.len() as u64);
            header.set_mode(0o644);
            builder
                .append_data(
                    &mut header,
                    "metadata.gz",
                    Cursor::new(metadata_bytes.as_slice()),
                )
                .expect("write metadata entry");

            let mut header = Header::new_gnu();
            header.set_size(data_bytes.len() as u64);
            header.set_mode(0o644);
            builder
                .append_data(
                    &mut header,
                    "data.tar.gz",
                    Cursor::new(data_bytes.as_slice()),
                )
                .expect("write data entry");

            builder.finish().expect("finish gem tar");
        }
        handle.flush().expect("flush gem file");
    }

    file
}

/// A gzipped tarball of `entries` (path, contents), as a `.crate` or an
/// npm package is shipped.
#[cfg(any(feature = "crates", feature = "npm", feature = "admin"))]
pub fn tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut tar_bytes = Vec::new();
    {
        let mut builder = Builder::new(&mut tar_bytes);
        for (path, contents) in entries {
            let mut header = Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            builder
                .append_data(&mut header, path, Cursor::new(*contents))
                .expect("append tarball entry");
        }
        builder.finish().expect("finish tarball");
    }
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&tar_bytes).expect("compress tarball");
    encoder.finish().expect("finalize tarball")
}

/// Size and hex SHA-256 of `path`, as the cache index records them.
#[cfg(feature = "gems")]
pub fn digest_file(path: &Path) -> (u64, String) {
    let bytes = std::fs::read(path).expect("read gem bytes");
    let size_bytes = bytes.len() as u64;
    let sha256 = hex::encode(Sha256::digest(&bytes));
    (size_bytes, sha256)
}
