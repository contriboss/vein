//! Request handlers for crates.io registry protocol

use std::borrow::Cow;
use std::sync::Arc;

use anyhow::{Context, Result};
use rama::http::{
    Body, Response, StatusCode,
    client::DefaultHttpWebClient,
    header::{self, HeaderValue},
};
use vein_adapter::{CacheBackend, FilesystemStorage};

use super::types::{IndexConfig, index_path, index_storage_path};
use crate::config::DelayPolicyConfig;
use crate::delay;
use crate::http_cache::{CacheOutcome, CachedTextOptions, MetaStoreMode, fetch_cached_text};
use crate::upstream::simple_get;

const CRATES_INDEX_BASE: &str = "https://index.crates.io";

// Not `const`: the test override consults thread-local state.
#[allow(clippy::missing_const_for_fn)]
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
    client: &DefaultHttpWebClient,
    path: &str,
    our_base: &str,
    storage: Arc<FilesystemStorage>,
    index: Arc<CacheBackend>,
    delay: &DelayPolicyConfig,
) -> Result<(Response<Body>, CacheOutcome)> {
    let index_base = crates_index_base();
    handle_sparse_index_from(
        client,
        path,
        our_base,
        storage,
        index,
        delay,
        index_base.as_ref(),
    )
    .await
}

async fn handle_sparse_index_from(
    client: &DefaultHttpWebClient,
    path: &str,
    our_base: &str,
    storage: Arc<FilesystemStorage>,
    index: Arc<CacheBackend>,
    delay: &DelayPolicyConfig,
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
    let Some(expected_path) = index_path(crate_name) else {
        return respond_text(StatusCode::BAD_REQUEST, "invalid crate name")
            .map(|r| (r, CacheOutcome::Pass));
    };
    let clean_path = path.trim_start_matches("/index/");

    if clean_path != expected_path {
        return respond_text(StatusCode::NOT_FOUND, "crate not found")
            .map(|r| (r, CacheOutcome::Pass));
    }

    let storage_path = index_storage_path(crate_name).context("invalid crate name")?;
    let meta_key = format!("crates:index:{crate_name}");
    let upstream_url = format!("{}/{}", index_base.trim_end_matches('/'), expected_path);

    let screen_index = Arc::clone(&index);
    let result =
        fetch_cached_text(
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
            |headers| async move {
                simple_get(client, &upstream_url, &headers, Some("text/plain")).await
            },
            |body| async move {
                Ok(delay::crates::filter_index(delay, &screen_index, crate_name, body).await)
            },
        )
        .await?;

    Ok((result.response, result.outcome))
}

/// Serve the sparse index config.json
fn serve_index_config(our_base: &str) -> Result<Response<Body>> {
    let config = IndexConfig {
        // Point downloads back to ourselves
        dl: format!("{our_base}/api/v1/crates/{{crate}}/{{version}}/download"),
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
pub mod test_override {
    use std::sync::MutexGuard;
    use std::sync::{Mutex, OnceLock};

    static OVERRIDE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    static CRATES_INDEX_BASE_OVERRIDE: OnceLock<Mutex<Option<String>>> = OnceLock::new();

    pub struct TestCratesIndexBaseGuard {
        _lock: MutexGuard<'static, ()>,
    }

    impl Drop for TestCratesIndexBaseGuard {
        fn drop(&mut self) {
            let slot = CRATES_INDEX_BASE_OVERRIDE.get_or_init(|| Mutex::new(None));
            *slot.lock().unwrap() = None;
        }
    }

    pub fn override_crates_index_base(base: impl Into<String>) -> TestCratesIndexBaseGuard {
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
mod tests;
