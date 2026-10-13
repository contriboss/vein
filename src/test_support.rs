//! Shared fixtures for unit tests: a scripted fake upstream over raw TCP and
//! the rustls provider install every HTTP client test needs.

use std::sync::Once;

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
