//! Request handlers for crates.io registry protocol

use std::borrow::Cow;
use std::sync::Arc;

use anyhow::{Context, Result};
use rama::http::{
    Body, Response, StatusCode,
    header::{self, HeaderValue},
};
use vein_adapter::{CacheBackend, FilesystemStorage};

use super::types::{IndexConfig, index_path};
use crate::http_cache::{CacheOutcome, CachedTextOptions, MetaStoreMode, fetch_cached_text};
use crate::upstream::simple_get;

const CRATES_INDEX_BASE: &str = "https://index.crates.io";

fn crates_index_base() -> Cow<'static, str> {
    #[cfg(test)]
    if let Some(base) = test_override::current_crates_index_base() {
        return Cow::Owned(base);
    }

    Cow::Borrowed(CRATES_INDEX_BASE)
}

/// Handle sparse index requests with caching
///
/// Path: `/index/{prefix}/{crate}` or `/index/config.json`
///
/// Caches index entries with ETag/Last-Modified revalidation.
/// Returns both the response and the cache outcome.
pub async fn handle_sparse_index(
    path: &str,
    our_base: &str,
    storage: Arc<FilesystemStorage>,
    index: Arc<CacheBackend>,
) -> Result<(Response<Body>, CacheOutcome)> {
    let index_base = crates_index_base();
    handle_sparse_index_from(path, our_base, storage, index, index_base.as_ref()).await
}

async fn handle_sparse_index_from(
    path: &str,
    our_base: &str,
    storage: Arc<FilesystemStorage>,
    index: Arc<CacheBackend>,
    index_base: &str,
) -> Result<(Response<Body>, CacheOutcome)> {
    // Handle config.json specially - serve our own
    if path == "/index/config.json" || path == "config.json" {
        return serve_index_config(our_base).map(|r| (r, CacheOutcome::Pass));
    }

    // Extract crate name from path
    // Path format: /index/{prefix}/{crate_name}
    let crate_name = path
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .context("invalid index path")?;

    // Validate the path matches expected prefix
    let expected_path = match index_path(crate_name) {
        Some(p) => p,
        None => {
            return respond_text(StatusCode::BAD_REQUEST, "invalid crate name")
                .map(|r| (r, CacheOutcome::Pass));
        }
    };
    let clean_path = path.trim_start_matches("/index/");

    if clean_path != expected_path {
        return respond_text(StatusCode::NOT_FOUND, "crate not found")
            .map(|r| (r, CacheOutcome::Pass));
    }

    let storage_path = format!("crates_index/{}", expected_path);
    let meta_key = format!("crates:index:{}", crate_name);
    let upstream_url = format!("{}/{}", index_base.trim_end_matches('/'), expected_path);

    let result = fetch_cached_text(
        storage.as_ref(),
        index.as_ref(),
        CachedTextOptions {
            storage_path: &storage_path,
            meta_key: &meta_key,
            content_type: "text/plain; charset=utf-8",
            cache_control: "public, max-age=60",
            include_content_length: false,
            meta_mode: MetaStoreMode::BestEffort,
            strip_transfer_encoding: false,
        },
        |headers| async move { simple_get(&upstream_url, &headers, Some("text/plain")).await },
        |body| async move { Ok(body) },
    )
    .await?;

    Ok((result.response, result.outcome))
}

/// Serve the sparse index config.json
fn serve_index_config(our_base: &str) -> Result<Response<Body>> {
    let config = IndexConfig {
        // Point downloads back to ourselves
        dl: format!("{}/api/v1/crates/{{crate}}/{{version}}/download", our_base),
        api: Some(our_base.to_string()),
        auth_required: None,
    };

    let body = serde_json::to_string_pretty(&config).context("serializing config")?;

    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=300"),
        )
        .body(Body::from(body))
        .context("building config response")
}

fn respond_text(status: StatusCode, message: &str) -> Result<Response<Body>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"))
        .body(Body::from(message.to_string()))
        .context("building text response")
}

#[cfg(test)]
pub(crate) mod test_override {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    static OVERRIDE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    static CRATES_INDEX_BASE_OVERRIDE: OnceLock<Mutex<Option<String>>> = OnceLock::new();

    pub(crate) struct TestCratesIndexBaseGuard {
        _lock: MutexGuard<'static, ()>,
    }

    impl Drop for TestCratesIndexBaseGuard {
        fn drop(&mut self) {
            let slot = CRATES_INDEX_BASE_OVERRIDE.get_or_init(|| Mutex::new(None));
            *slot.lock().unwrap() = None;
        }
    }

    pub(crate) fn override_crates_index_base(base: impl Into<String>) -> TestCratesIndexBaseGuard {
        let lock = OVERRIDE_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let slot = CRATES_INDEX_BASE_OVERRIDE.get_or_init(|| Mutex::new(None));
        *slot.lock().unwrap() = Some(base.into());
        TestCratesIndexBaseGuard { _lock: lock }
    }

    pub(super) fn current_crates_index_base() -> Option<String> {
        let slot = CRATES_INDEX_BASE_OVERRIDE.get_or_init(|| Mutex::new(None));
        slot.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rama::http::body::util::BodyExt;
    use rama::tls::rustls::dep::rustls;
    use std::sync::{Arc, Once};
    use tempfile::tempdir;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    use vein_adapter::{CacheBackend, CacheBackendTrait, FilesystemStorage};

    #[test]
    fn test_serve_index_config() {
        let response = serve_index_config("http://localhost:8346").unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn test_handle_sparse_index_caches_and_revalidates() {
        install_rustls_provider();

        let temp_dir = tempdir().unwrap();
        let storage = Arc::new(FilesystemStorage::new(temp_dir.path().join("cache")));
        storage.prepare().await.unwrap();
        let index = Arc::new(CacheBackend::connect_memory().await.unwrap());

        let body = b"{\"name\":\"serde\"}\n";
        let (upstream_base, server) = spawn_sequence_server(vec![
            raw_response(
                "200 OK",
                &[
                    ("Content-Type", "text/plain"),
                    ("ETag", "\"serde-v1\""),
                    ("Last-Modified", "Wed, 01 Jan 2025 00:00:00 GMT"),
                ],
                body,
            ),
            raw_response("304 Not Modified", &[], &[]),
        ])
        .await;

        let (response, outcome) = handle_sparse_index_from(
            "/index/se/rd/serde",
            "http://localhost:8346",
            storage.clone(),
            index.clone(),
            &upstream_base,
        )
        .await
        .unwrap();
        assert_eq!(outcome, CacheOutcome::Miss);
        assert_eq!(body_bytes(response).await, body);

        let (response, outcome) = handle_sparse_index_from(
            "/index/se/rd/serde",
            "http://localhost:8346",
            storage.clone(),
            index.clone(),
            &upstream_base,
        )
        .await
        .unwrap();
        assert_eq!(outcome, CacheOutcome::Revalidated);
        assert_eq!(body_bytes(response).await, body);

        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].starts_with("get /se/rd/serde http/1.1"));
        assert!(requests[0].contains("accept: text/plain"));
        assert!(requests[1].contains("if-none-match: \"serde-v1\""));
        assert!(requests[1].contains("if-modified-since: wed, 01 jan 2025 00:00:00 gmt"));

        assert!(storage.resolve("crates_index/se/rd/serde").exists());
        assert!(
            index
                .catalog_meta_get("crates:index:serde")
                .await
                .unwrap()
                .is_some()
        );
    }

    #[cfg(feature = "sqlite")]
    fn install_rustls_provider() {
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        });
    }

    #[cfg(feature = "sqlite")]
    async fn spawn_sequence_server(
        responses: Vec<Vec<u8>>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let handle = tokio::spawn(async move {
            let mut requests = Vec::new();
            for response in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let request = read_http_request(&mut socket).await;
                requests.push(request);
                socket.write_all(&response).await.unwrap();
            }
            requests
        });

        (format!("http://{}", addr), handle)
    }

    #[cfg(feature = "sqlite")]
    fn raw_response(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
        let mut response = format!("HTTP/1.1 {}\r\n", status);
        for (name, value) in headers {
            response.push_str(name);
            response.push_str(": ");
            response.push_str(value);
            response.push_str("\r\n");
        }
        response.push_str(&format!("Content-Length: {}\r\n", body.len()));
        response.push_str("Connection: close\r\n\r\n");

        let mut bytes = response.into_bytes();
        bytes.extend_from_slice(body);
        bytes
    }

    #[cfg(feature = "sqlite")]
    async fn read_http_request(socket: &mut tokio::net::TcpStream) -> String {
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

    #[cfg(feature = "sqlite")]
    async fn body_bytes(response: Response<Body>) -> Vec<u8> {
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec()
    }
}
