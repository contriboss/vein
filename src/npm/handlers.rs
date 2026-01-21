//! NPM registry request handlers
//!
//! Handles package metadata and tarball requests with caching.

use std::sync::Arc;

use anyhow::{Context, Result};
use rama::http::{
    Body, Method, Request, Response, StatusCode,
    body::util::BodyExt,
    client::EasyHttpWebClient,
    header::{self, HeaderMap, HeaderName, HeaderValue},
};
use rama::telemetry::tracing::warn;
use rama::Service;
use serde_json::Value as JsonValue;
use vein_adapter::{AssetKind, CacheBackend, CacheBackendTrait, FilesystemStorage};

use super::types::NpmPackageRequest;
use crate::http_cache::{fetch_cached_text, CacheOutcome, MetaStoreMode};
use crate::proxy::{cache as proxy_cache, types::CacheableRequest};

const UA: &str = concat!("vein/", env!("CARGO_PKG_VERSION"));
const NPM_REGISTRY: &str = "https://registry.npmjs.org";

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
    let method = req.method().clone();
    let path = req.uri().path().to_string();

    if path.starts_with("/-/") {
        return handle_npm_api(req).await;
    }

    if method != Method::GET && method != Method::HEAD {
        return respond_error(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed");
    }

    // Parse the npm request
    let npm_req = match NpmPackageRequest::from_path(&path) {
        Some(r) => r,
        None => {
            return respond_error(
                StatusCode::BAD_REQUEST,
                "Invalid npm package path",
            );
        }
    };

    if npm_req.is_tarball {
        handle_tarball_download(&npm_req, storage, index).await
    } else {
        handle_package_metadata(&npm_req, our_base, storage, index).await
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
) -> Result<(Response<Body>, CacheOutcome)> {
    let storage_path = npm_req.storage_path();
    let meta_key = npm_req.meta_key();

    // URL-encode the package name for scoped packages
    let encoded_name = npm_req.name.replace('/', "%2f");
    let upstream_url = if let Some(version) = npm_req.version.as_deref() {
        format!("{}/{}/{}", NPM_REGISTRY, encoded_name, version)
    } else {
        format!("{}/{}", NPM_REGISTRY, encoded_name)
    };

    let our_base = our_base.to_string();

    let result = fetch_cached_text(
        storage.as_ref(),
        index.as_ref(),
        &storage_path,
        &meta_key,
        "application/json",
        "public, max-age=60",  // Short TTL for metadata (can change)
        true,
        MetaStoreMode::BestEffort,
        false,
        |headers| async move { fetch_with_headers(&upstream_url, &headers, Some("application/json")).await },
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
) -> Result<(Response<Body>, CacheOutcome)> {
    let storage_path = npm_req.storage_path();

    // URL-encode the package name for scoped packages
    let encoded_name = npm_req.name.replace('/', "%2f");
    let tarball_name = npm_req.tarball_name.as_deref().unwrap_or("package.tgz");
    let upstream_url = format!("{}/{}/-/{}", NPM_REGISTRY, encoded_name, tarball_name);

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
        &cacheable,
        index,
        storage,
        response,
        temp_file,
        had_cache,
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
async fn handle_npm_api(req: Request<Body>) -> Result<(Response<Body>, CacheOutcome)> {
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
    let upstream_url = format!("{}{}", NPM_REGISTRY, path_and_query);

    let body_bytes = body
        .collect()
        .await
        .context("reading npm api request body")?
        .to_bytes();

    let mut builder = Request::builder()
        .method(parts.method)
        .uri(upstream_url);

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
    let response = client.serve(request).await
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

    let mut builder = Request::builder()
        .method(Method::GET)
        .uri(url);

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

    let request = builder.body(Body::empty()).context("building upstream request")?;

    client.serve(request).await
        .map_err(|e| anyhow::anyhow!("upstream request failed: {e}"))
}

/// Transform package metadata to point tarball URLs to our proxy
fn transform_metadata(body: &[u8], our_base: &str) -> Result<Vec<u8>> {
    let mut metadata: JsonValue = serde_json::from_slice(body)
        .context("parsing npm metadata")?;

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
mod tests {
    use super::*;

    fn make_request(headers: &[(&str, &str)]) -> Request<Body> {
        let mut builder = Request::builder()
            .method(Method::GET)
            .uri("/lodash");

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

        let result = transform_metadata(
            metadata.as_bytes(),
            "http://localhost:8346",
        ).unwrap();

        let transformed: JsonValue = serde_json::from_slice(&result).unwrap();
        let tarball = transformed["versions"]["4.17.21"]["dist"]["tarball"].as_str().unwrap();
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

        let result = transform_metadata(
            metadata.as_bytes(),
            "http://localhost:8346",
        ).unwrap();

        let transformed: JsonValue = serde_json::from_slice(&result).unwrap();
        let tarball = transformed["dist"]["tarball"].as_str().unwrap();
        assert_eq!(tarball, "http://localhost:8346/lodash/-/lodash-4.17.21.tgz");
    }
}
