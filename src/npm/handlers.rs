//! NPM registry request handlers
//!
//! Handles package metadata and tarball requests with caching.

use std::borrow::Cow;
use std::sync::Arc;

use anyhow::{Context, Result};
use rama::Service;
use rama::http::{
    Body, Method, Request, Response, StatusCode,
    body::util::BodyExt,
    client::EasyHttpWebClient,
    header::{self, HeaderMap, HeaderName, HeaderValue},
};
use rama::telemetry::tracing::warn;
use serde_json::Value as JsonValue;
use vein_adapter::{AssetKind, CacheBackend, CacheBackendTrait, FilesystemStorage};

use super::types::NpmPackageRequest;
use crate::http_cache::{CacheOutcome, MetaStoreMode, fetch_cached_text};
use crate::proxy::{cache as proxy_cache, types::CacheableRequest};

const NPM_REGISTRY_BASE: &str = "https://registry.npmjs.org";
const UA: &str = concat!("vein/", env!("CARGO_PKG_VERSION"));

fn npm_registry_base() -> Cow<'static, str> {
    #[cfg(test)]
    if let Some(base) = test_override::current_npm_registry_base() {
        return Cow::Owned(base);
    }

    Cow::Borrowed(NPM_REGISTRY_BASE)
}

/// Check if a request is from an npm client (header-based detection)
pub fn is_npm_request(req: &Request<Body>) -> bool {
    // Check for npm-command header (most reliable)
    if req.headers().contains_key("npm-command") {
        return true;
    }

    // Check User-Agent for npm client
    if let Some(ua) = req.headers().get(header::USER_AGENT) {
        if let Ok(ua_str) = ua.to_str() {
            if ua_str.starts_with("npm/") || ua_str.contains(" npm/") {
                return true;
            }
        }
    }

    // Check Accept header for npm-specific types
    if let Some(accept) = req.headers().get(header::ACCEPT) {
        if let Ok(accept_str) = accept.to_str() {
            if accept_str.contains("application/vnd.npm") {
                return true;
            }
        }
    }

    false
}

/// Handle an npm registry request
///
/// Routes to metadata or tarball handlers based on path.
pub async fn handle_npm_request(
    req: Request<Body>,
    our_base: &str,
    storage: Arc<FilesystemStorage>,
    index: Arc<CacheBackend>,
) -> Result<(Response<Body>, CacheOutcome)> {
    let registry_base = npm_registry_base();
    handle_npm_request_from(req, our_base, storage, index, registry_base.as_ref()).await
}

async fn handle_npm_request_from(
    req: Request<Body>,
    our_base: &str,
    storage: Arc<FilesystemStorage>,
    index: Arc<CacheBackend>,
    registry_base: &str,
) -> Result<(Response<Body>, CacheOutcome)> {
    let method = req.method().clone();
    let path = req.uri().path().to_string();

    if path.starts_with("/-/") {
        return handle_npm_api(req, registry_base).await;
    }

    if method != Method::GET && method != Method::HEAD {
        return respond_error(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed");
    }

    // Parse the npm request
    let npm_req = match NpmPackageRequest::from_path(&path) {
        Some(r) => r,
        None => {
            return respond_error(StatusCode::BAD_REQUEST, "Invalid npm package path");
        }
    };

    if npm_req.is_tarball {
        handle_tarball_download(&npm_req, storage, index, registry_base).await
    } else {
        handle_package_metadata(&npm_req, our_base, storage, index, registry_base).await
    }
}

/// Handle package metadata request
///
/// Fetches from registry.npmjs.org with short TTL caching.
/// Transforms tarball URLs to point to our proxy.
async fn handle_package_metadata(
    npm_req: &NpmPackageRequest,
    our_base: &str,
    storage: Arc<FilesystemStorage>,
    index: Arc<CacheBackend>,
    registry_base: &str,
) -> Result<(Response<Body>, CacheOutcome)> {
    let storage_path = npm_req.storage_path();
    let meta_key = npm_req.meta_key();
    let registry_base = registry_base.trim_end_matches('/').to_string();

    // URL-encode the package name for scoped packages
    let encoded_name = npm_req.name.replace('/', "%2f");
    let upstream_url = if let Some(version) = npm_req.version.as_deref() {
        format!("{}/{}/{}", registry_base, encoded_name, version)
    } else {
        format!("{}/{}", registry_base, encoded_name)
    };

    let our_base = our_base.to_string();

    let result = fetch_cached_text(
        storage.as_ref(),
        index.as_ref(),
        &storage_path,
        &meta_key,
        "application/json",
        "public, max-age=60", // Short TTL for metadata (can change)
        true,
        MetaStoreMode::BestEffort,
        false,
        |headers| async move {
            fetch_with_headers(&upstream_url, &headers, Some("application/json")).await
        },
        move |body| async move {
            // Transform tarball URLs to point to our proxy
            transform_metadata(&body, &our_base)
        },
    )
    .await?;

    Ok((result.response, result.outcome))
}

/// Handle tarball download request
///
/// Fetches from registry.npmjs.org with permanent caching.
async fn handle_tarball_download(
    npm_req: &NpmPackageRequest,
    storage: Arc<FilesystemStorage>,
    index: Arc<CacheBackend>,
    registry_base: &str,
) -> Result<(Response<Body>, CacheOutcome)> {
    let storage_path = npm_req.storage_path();
    let registry_base = registry_base.trim_end_matches('/').to_string();

    // URL-encode the package name for scoped packages
    let encoded_name = npm_req.name.replace('/', "%2f");
    let tarball_name = npm_req.tarball_name.as_deref().unwrap_or("package.tgz");
    let upstream_url = format!("{}/{}/-/{}", registry_base, encoded_name, tarball_name);

    let cacheable = CacheableRequest {
        kind: AssetKind::NpmPackage,
        name: npm_req.name.clone(),
        version: npm_req
            .version
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        platform: None,
        file_name: tarball_name.to_string(),
        relative_path: storage_path,
    };

    let mut had_cache = false;
    if let Some(entry) = index.get(&cacheable.asset_key()).await? {
        match proxy_cache::serve_cached(&cacheable, entry, storage.as_ref()).await {
            Ok(resp) => return Ok((resp, CacheOutcome::Hit)),
            Err(err) => {
                warn!(
                    error = %err,
                    "failed to serve cached npm tarball, refetching"
                );
                had_cache = true;
            }
        }
    }

    let response = fetch_with_headers(&upstream_url, &HeaderMap::new(), None).await?;
    if !response.status().is_success() {
        let forwarded = forward_response(response).await?;
        return Ok((forwarded, CacheOutcome::Pass));
    }

    let temp_file = storage
        .create_temp_writer(&cacheable.relative_path)
        .await
        .context("creating npm cache temp file")?;

    let response = proxy_cache::run_cache_miss_flow(
        &cacheable, index, storage, response, temp_file, had_cache,
    )
    .await?;

    let outcome = if had_cache {
        CacheOutcome::Revalidated
    } else {
        CacheOutcome::Miss
    };

    Ok((response, outcome))
}

/// Handle npm API endpoints (pass-through)
async fn handle_npm_api(
    req: Request<Body>,
    registry_base: &str,
) -> Result<(Response<Body>, CacheOutcome)> {
    let path = req.uri().path().to_string();
    if path == "/-/ping" && req.method() == Method::GET {
        let response = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))?;
        return Ok((response, CacheOutcome::Pass));
    }

    let (parts, body) = req.into_parts();
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or(parts.uri.path());
    let upstream_url = format!("{}{}", registry_base.trim_end_matches('/'), path_and_query);

    let body_bytes = body
        .collect()
        .await
        .context("reading npm api request body")?
        .to_bytes();

    let mut builder = Request::builder().method(parts.method).uri(upstream_url);

    {
        let headers = builder.headers_mut().context("getting headers")?;
        copy_request_headers(&parts.headers, headers);
        headers.insert(header::USER_AGENT, HeaderValue::from_static(UA));
        headers.insert(
            header::CONTENT_LENGTH,
            HeaderValue::from_str(&body_bytes.len().to_string())?,
        );
    }

    let request = builder
        .body(Body::from(body_bytes))
        .context("building npm api request")?;

    let client = EasyHttpWebClient::default();
    let response = client
        .serve(request)
        .await
        .map_err(|e| anyhow::anyhow!("npm api request failed: {e}"))?;

    let forwarded = forward_response(response).await?;
    Ok((forwarded, CacheOutcome::Pass))
}

/// Fetch from upstream with optional headers
async fn fetch_with_headers(
    url: &str,
    extra_headers: &HeaderMap,
    accept: Option<&str>,
) -> Result<Response<Body>> {
    let client = EasyHttpWebClient::default();

    let mut builder = Request::builder().method(Method::GET).uri(url);

    {
        let headers = builder.headers_mut().context("getting headers")?;
        headers.insert(header::USER_AGENT, HeaderValue::from_static(UA));
        for (name, value) in extra_headers {
            headers.insert(name, value.clone());
        }
        if let Some(accept) = accept
            && !headers.contains_key(header::ACCEPT)
        {
            headers.insert(header::ACCEPT, HeaderValue::from_str(accept)?);
        }
    }

    let request = builder
        .body(Body::empty())
        .context("building upstream request")?;

    client
        .serve(request)
        .await
        .map_err(|e| anyhow::anyhow!("upstream request failed: {e}"))
}

/// Transform package metadata to point tarball URLs to our proxy
fn transform_metadata(body: &[u8], our_base: &str) -> Result<Vec<u8>> {
    let mut metadata: JsonValue = serde_json::from_slice(body).context("parsing npm metadata")?;

    // Transform top-level dist.tarball (version-specific metadata)
    if let Some(dist) = metadata.get_mut("dist").and_then(|d| d.as_object_mut()) {
        if let Some(tarball) = dist.get("tarball").and_then(|t| t.as_str()) {
            let new_tarball = tarball
                .replace("https://registry.npmjs.org", our_base)
                .replace("http://registry.npmjs.org", our_base);
            dist.insert("tarball".to_string(), JsonValue::String(new_tarball));
        }
    }

    // Transform dist.tarball URLs in all versions
    if let Some(versions) = metadata.get_mut("versions").and_then(|v| v.as_object_mut()) {
        for (_version, version_data) in versions.iter_mut() {
            if let Some(dist) = version_data.get_mut("dist").and_then(|d| d.as_object_mut()) {
                if let Some(tarball) = dist.get("tarball").and_then(|t| t.as_str()) {
                    // Replace registry.npmjs.org with our base URL
                    let new_tarball = tarball
                        .replace("https://registry.npmjs.org", our_base)
                        .replace("http://registry.npmjs.org", our_base);
                    dist.insert("tarball".to_string(), JsonValue::String(new_tarball));
                }
            }
        }
    }

    serde_json::to_vec(&metadata).context("serializing transformed metadata")
}

async fn forward_response(response: Response<Body>) -> Result<Response<Body>> {
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .into_body()
        .collect()
        .await
        .context("reading npm response body")?
        .to_bytes();

    let mut builder = Response::builder().status(status);
    if let Some(h) = builder.headers_mut() {
        for (name, value) in headers.iter() {
            if name != header::TRANSFER_ENCODING {
                h.insert(name, value.clone());
            }
        }
    }

    Ok(builder.body(Body::from(body))?)
}

fn copy_request_headers(source: &HeaderMap, target: &mut HeaderMap) {
    for (name, value) in source.iter() {
        if is_hop_header(name) || name == header::HOST || name == header::CONTENT_LENGTH {
            continue;
        }
        target.insert(name, value.clone());
    }
}

fn is_hop_header(name: &HeaderName) -> bool {
    name == header::CONNECTION
        || name == header::KEEP_ALIVE
        || name == header::PROXY_AUTHENTICATE
        || name == header::PROXY_AUTHORIZATION
        || name == header::TE
        || name == header::TRAILER
        || name == header::TRANSFER_ENCODING
        || name == header::UPGRADE
}

fn respond_error(status: StatusCode, message: &str) -> Result<(Response<Body>, CacheOutcome)> {
    let body = serde_json::json!({
        "error": message
    });
    let response = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body)?))?;
    Ok((response, CacheOutcome::Pass))
}

#[cfg(test)]
pub(crate) mod test_override {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    static OVERRIDE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    static NPM_REGISTRY_BASE_OVERRIDE: OnceLock<Mutex<Option<String>>> = OnceLock::new();

    pub(crate) struct TestNpmRegistryBaseGuard {
        _lock: MutexGuard<'static, ()>,
    }

    impl Drop for TestNpmRegistryBaseGuard {
        fn drop(&mut self) {
            let slot = NPM_REGISTRY_BASE_OVERRIDE.get_or_init(|| Mutex::new(None));
            *slot.lock().unwrap() = None;
        }
    }

    pub(crate) fn override_npm_registry_base(base: impl Into<String>) -> TestNpmRegistryBaseGuard {
        let lock = OVERRIDE_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let slot = NPM_REGISTRY_BASE_OVERRIDE.get_or_init(|| Mutex::new(None));
        *slot.lock().unwrap() = Some(base.into());
        TestNpmRegistryBaseGuard { _lock: lock }
    }

    pub(super) fn current_npm_registry_base() -> Option<String> {
        let slot = NPM_REGISTRY_BASE_OVERRIDE.get_or_init(|| Mutex::new(None));
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

    fn make_request(headers: &[(&str, &str)]) -> Request<Body> {
        let mut builder = Request::builder().method(Method::GET).uri("/lodash");

        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }

        builder.body(Body::empty()).unwrap()
    }

    #[test]
    fn test_is_npm_request_with_npm_command() {
        let req = make_request(&[("npm-command", "install")]);
        assert!(is_npm_request(&req));
    }

    #[test]
    fn test_is_npm_request_with_npm_user_agent() {
        let req = make_request(&[("user-agent", "npm/9.0.0 node/v18.0.0")]);
        assert!(is_npm_request(&req));
    }

    #[test]
    fn test_is_npm_request_with_vnd_accept() {
        let req = make_request(&[("accept", "application/vnd.npm.install-v1+json")]);
        assert!(is_npm_request(&req));
    }

    #[test]
    fn test_is_npm_request_without_npm_headers() {
        let req = make_request(&[("user-agent", "curl/7.0.0")]);
        assert!(!is_npm_request(&req));
    }

    #[test]
    fn test_transform_metadata() {
        let metadata = r#"{
            "name": "lodash",
            "versions": {
                "4.17.21": {
                    "dist": {
                        "tarball": "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz",
                        "shasum": "abc123"
                    }
                }
            }
        }"#;

        let result = transform_metadata(metadata.as_bytes(), "http://localhost:8346").unwrap();

        let transformed: JsonValue = serde_json::from_slice(&result).unwrap();
        let tarball = transformed["versions"]["4.17.21"]["dist"]["tarball"]
            .as_str()
            .unwrap();
        assert_eq!(tarball, "http://localhost:8346/lodash/-/lodash-4.17.21.tgz");
    }

    #[test]
    fn test_transform_metadata_version_only() {
        let metadata = r#"{
            "name": "lodash",
            "version": "4.17.21",
            "dist": {
                "tarball": "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz"
            }
        }"#;

        let result = transform_metadata(metadata.as_bytes(), "http://localhost:8346").unwrap();

        let transformed: JsonValue = serde_json::from_slice(&result).unwrap();
        let tarball = transformed["dist"]["tarball"].as_str().unwrap();
        assert_eq!(tarball, "http://localhost:8346/lodash/-/lodash-4.17.21.tgz");
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn test_handle_npm_metadata_caches_and_revalidates() {
        install_rustls_provider();

        let temp_dir = tempdir().unwrap();
        let storage = Arc::new(FilesystemStorage::new(temp_dir.path().join("cache")));
        storage.prepare().await.unwrap();
        let index = Arc::new(CacheBackend::connect_memory().await.unwrap());

        let metadata = br#"{
            "name": "lodash",
            "versions": {
                "4.17.21": {
                    "dist": {
                        "tarball": "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz"
                    }
                }
            }
        }"#;
        let (registry_base, server) = spawn_sequence_server(vec![
            raw_response(
                "200 OK",
                &[
                    ("Content-Type", "application/json"),
                    ("ETag", "\"lodash-meta-v1\""),
                    ("Last-Modified", "Wed, 01 Jan 2025 00:00:00 GMT"),
                ],
                metadata,
            ),
            raw_response("304 Not Modified", &[], &[]),
        ])
        .await;

        let request = Request::builder()
            .method(Method::GET)
            .uri("/lodash")
            .body(Body::empty())
            .unwrap();
        let (response, outcome) = handle_npm_request_from(
            request,
            "http://localhost:8346",
            storage.clone(),
            index.clone(),
            &registry_base,
        )
        .await
        .unwrap();
        assert_eq!(outcome, CacheOutcome::Miss);
        let response_body: JsonValue = serde_json::from_slice(&body_bytes(response).await).unwrap();
        let tarball = response_body["versions"]["4.17.21"]["dist"]["tarball"]
            .as_str()
            .unwrap();
        assert_eq!(tarball, "http://localhost:8346/lodash/-/lodash-4.17.21.tgz");

        let request = Request::builder()
            .method(Method::GET)
            .uri("/lodash")
            .body(Body::empty())
            .unwrap();
        let (response, outcome) = handle_npm_request_from(
            request,
            "http://localhost:8346",
            storage.clone(),
            index.clone(),
            &registry_base,
        )
        .await
        .unwrap();
        assert_eq!(outcome, CacheOutcome::Revalidated);
        let response_body: JsonValue = serde_json::from_slice(&body_bytes(response).await).unwrap();
        let tarball = response_body["versions"]["4.17.21"]["dist"]["tarball"]
            .as_str()
            .unwrap();
        assert_eq!(tarball, "http://localhost:8346/lodash/-/lodash-4.17.21.tgz");

        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].starts_with("get /lodash http/1.1"));
        assert!(requests[1].contains("if-none-match: \"lodash-meta-v1\""));
        assert!(requests[1].contains("if-modified-since: wed, 01 jan 2025 00:00:00 gmt"));

        assert!(storage.resolve("npm_index/lodash/metadata.json").exists());
        assert!(
            index
                .catalog_meta_get("npm:metadata:lodash")
                .await
                .unwrap()
                .is_some()
        );
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn test_handle_npm_tarball_hits_cache_on_second_request() {
        install_rustls_provider();

        let temp_dir = tempdir().unwrap();
        let storage = Arc::new(FilesystemStorage::new(temp_dir.path().join("cache")));
        storage.prepare().await.unwrap();
        let index = Arc::new(CacheBackend::connect_memory().await.unwrap());

        let tarball = b"fake tgz bytes";
        let (registry_base, server) = spawn_sequence_server(vec![raw_response(
            "200 OK",
            &[("Content-Type", "application/octet-stream")],
            tarball,
        )])
        .await;

        let request = Request::builder()
            .method(Method::GET)
            .uri("/lodash/-/lodash-4.17.21.tgz")
            .body(Body::empty())
            .unwrap();
        let (response, outcome) = handle_npm_request_from(
            request,
            "http://localhost:8346",
            storage.clone(),
            index.clone(),
            &registry_base,
        )
        .await
        .unwrap();
        assert_eq!(outcome, CacheOutcome::Miss);
        assert_eq!(body_bytes(response).await, tarball);

        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("get /lodash/-/lodash-4.17.21.tgz http/1.1"));

        let request = Request::builder()
            .method(Method::GET)
            .uri("/lodash/-/lodash-4.17.21.tgz")
            .body(Body::empty())
            .unwrap();
        let (response, outcome) = handle_npm_request_from(
            request,
            "http://localhost:8346",
            storage.clone(),
            index.clone(),
            &registry_base,
        )
        .await
        .unwrap();
        assert_eq!(outcome, CacheOutcome::Hit);
        assert_eq!(body_bytes(response).await, tarball);

        assert!(storage.resolve("npm/lodash/lodash-4.17.21.tgz").exists());
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
