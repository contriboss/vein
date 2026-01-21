//! Request handlers for crates.io registry protocol

use std::sync::Arc;

use anyhow::{Context, Result};
use rama::http::{
    client::EasyHttpWebClient,
    header::{self, HeaderMap, HeaderValue},
    Body, Method, Request, Response, StatusCode,
};
use rama::Service;
use vein_adapter::{CacheBackend, FilesystemStorage};

use crate::http_cache::{fetch_cached_text, CacheOutcome, MetaStoreMode};
use super::types::{index_path, IndexConfig};

const UA: &str = concat!("vein/", env!("CARGO_PKG_VERSION"));

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
        None => return respond_text(StatusCode::BAD_REQUEST, "invalid crate name").map(|r| (r, CacheOutcome::Pass)),
    };
    let clean_path = path.trim_start_matches("/index/");

    if clean_path != expected_path {
        return respond_text(StatusCode::NOT_FOUND, "crate not found").map(|r| (r, CacheOutcome::Pass));
    }

    let storage_path = format!("crates_index/{}", expected_path);
    let meta_key = format!("crates:index:{}", crate_name);
    let upstream_url = format!("https://index.crates.io/{}", expected_path);

    let result = fetch_cached_text(
        storage.as_ref(),
        index.as_ref(),
        &storage_path,
        &meta_key,
        "text/plain; charset=utf-8",
        "public, max-age=60",
        false,
        MetaStoreMode::BestEffort,
        false,
        |headers| async move { fetch_with_headers(&upstream_url, &headers).await },
        |body| async move { Ok(body) },
    )
    .await?;

    Ok((result.response, result.outcome))
}

/// Fetch from upstream with optional headers
async fn fetch_with_headers(url: &str, extra_headers: &HeaderMap) -> Result<Response<Body>> {
    let client = EasyHttpWebClient::default();

    let mut builder = Request::builder()
        .method(Method::GET)
        .uri(url);

    {
        let headers = builder.headers_mut().context("getting headers")?;
        headers.insert(header::USER_AGENT, HeaderValue::from_static(UA));
        headers.insert(header::ACCEPT, HeaderValue::from_static("text/plain"));
        for (name, value) in extra_headers {
            headers.insert(name, value.clone());
        }
    }

    let request = builder.body(Body::empty()).context("building upstream request")?;

    client.serve(request).await
        .map_err(|e| anyhow::anyhow!("upstream request failed: {e}"))
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
        .header(header::CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .header(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=300"))
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
mod tests {
    use super::*;

    #[test]
    fn test_serve_index_config() {
        let response = serve_index_config("http://localhost:8346").unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
