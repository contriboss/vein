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
    client::DefaultHttpWebClient,
    header::{self, HeaderMap, HeaderName, HeaderValue},
};
use rama::telemetry::tracing::warn;
use serde_json::Value as JsonValue;
use vein_adapter::{AssetKind, CacheBackend, CacheBackendTrait, FilesystemStorage};

use super::types::NpmPackageRequest;
use crate::config::DelayPolicyConfig;
use crate::delay::{self, BLOCKED};
use crate::http_cache::{CacheOutcome, CachedTextOptions, MetaStoreMode, fetch_cached_text};
use crate::proxy::{cache as proxy_cache, types::CacheableRequest};
use crate::upstream::{UA, buffer_response, simple_get};

const NPM_REGISTRY_BASE: &str = "https://registry.npmjs.org";

// Not `const`: the test override consults thread-local state.
#[allow(clippy::missing_const_for_fn)]
fn npm_registry_base() -> Cow<'static, str> {
    #[cfg(test)]
    if let Some(base) = test_override::current_npm_registry_base() {
        return Cow::Owned(base);
    }

    Cow::Borrowed(NPM_REGISTRY_BASE)
}

/// Check if a request is from an npm client: a tarball path, or npm headers.
///
/// bun sends neither `npm-command` nor an npm `Accept` type when it downloads
/// a tarball (`User-Agent: Bun/1.4.3`, `Accept: */*`), so its downloads are
/// recognized by path and its metadata requests by user agent. pnpm and
/// yarn name npm in their user agents (`pnpm/9 npm/? node/...`).
pub fn is_npm_request(req: &Request<Body>) -> bool {
    if is_tarball_path(&req.uri().path_or_root()) {
        return true;
    }

    // Check for npm-command header (most reliable)
    if req.headers().contains_key("npm-command") {
        return true;
    }

    // Check User-Agent for npm-compatible clients
    if let Some(ua) = req.headers().get(header::USER_AGENT)
        && let Ok(ua_str) = ua.to_str()
        && (ua_str.starts_with("npm/") || ua_str.contains(" npm/") || ua_str.starts_with("Bun/"))
    {
        return true;
    }

    // Check Accept header for npm-specific types
    if let Some(accept) = req.headers().get(header::ACCEPT)
        && let Ok(accept_str) = accept.to_str()
        && accept_str.contains("application/vnd.npm")
    {
        return true;
    }

    false
}

/// npm's tarball URLs, `/<name>/-/<file>.tgz` and `/@scope/<name>/-/<file>.tgz`.
/// No gem or crate path has a `-` segment followed by a `.tgz` file.
fn is_tarball_path(path: &str) -> bool {
    let Some(rest) = path.strip_prefix('/') else {
        return false;
    };
    let tgz = |file: &str| {
        std::path::Path::new(file)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("tgz"))
    };
    let segments: Vec<&str> = rest.split('/').collect();
    match segments.as_slice() {
        [name, "-", file] => !name.is_empty() && !name.starts_with('@') && tgz(file),
        [scope, name, "-", file] => {
            scope.len() > 1 && scope.starts_with('@') && !name.is_empty() && tgz(file)
        }
        _ => false,
    }
}

/// Handle an npm registry request
///
/// Routes to metadata or tarball handlers based on path.
pub async fn handle_npm_request(
    client: &DefaultHttpWebClient,
    req: Request<Body>,
    our_base: &str,
    storage: Arc<FilesystemStorage>,
    index: Arc<CacheBackend>,
    delay: &DelayPolicyConfig,
) -> Result<(Response<Body>, CacheOutcome)> {
    let registry_base = npm_registry_base();
    handle_npm_request_from(
        client,
        req,
        our_base,
        storage,
        index,
        delay,
        registry_base.as_ref(),
    )
    .await
}

async fn handle_npm_request_from(
    client: &DefaultHttpWebClient,
    req: Request<Body>,
    our_base: &str,
    storage: Arc<FilesystemStorage>,
    index: Arc<CacheBackend>,
    delay: &DelayPolicyConfig,
    registry_base: &str,
) -> Result<(Response<Body>, CacheOutcome)> {
    let method = req.method().clone();
    let path = req.uri().path_or_root().into_owned();

    if path.starts_with("/-/") {
        return handle_npm_api(client, req, registry_base).await;
    }

    if method != Method::GET && method != Method::HEAD {
        return respond_error(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed");
    }

    // Parse the npm request
    let Some(npm_req) = NpmPackageRequest::from_path(&path) else {
        return respond_error(StatusCode::BAD_REQUEST, "Invalid npm package path");
    };

    if npm_req.is_tarball {
        handle_tarball_download(client, &npm_req, storage, index, delay, registry_base).await
    } else {
        handle_package_metadata(
            client,
            &npm_req,
            our_base,
            storage,
            index,
            delay,
            registry_base,
        )
        .await
    }
}

/// Handle package metadata request
///
/// Fetches from registry.npmjs.org with short TTL caching.
/// Transforms tarball URLs to point to our proxy.
async fn handle_package_metadata(
    client: &DefaultHttpWebClient,
    npm_req: &NpmPackageRequest,
    our_base: &str,
    storage: Arc<FilesystemStorage>,
    index: Arc<CacheBackend>,
    delay: &DelayPolicyConfig,
    registry_base: &str,
) -> Result<(Response<Body>, CacheOutcome)> {
    let storage_path = npm_req.storage_path();
    let meta_key = npm_req.meta_key();
    let registry_base = registry_base.trim_end_matches('/').to_string();

    // URL-encode the package name for scoped packages
    let encoded_name = npm_req.name.replace('/', "%2f");
    let upstream_url = if let Some(version) = npm_req.version.as_deref() {
        format!("{registry_base}/{encoded_name}/{version}")
    } else {
        format!("{registry_base}/{encoded_name}")
    };

    let our_base = our_base.to_string();
    // A packument lists every version; a per-version manifest passes through.
    let screened = npm_req
        .version
        .is_none()
        .then(|| (index.clone(), npm_req.name.clone()));

    let result = fetch_cached_text(
        storage.as_ref(),
        index.as_ref(),
        CachedTextOptions {
            storage_path: &storage_path,
            meta_key: &meta_key,
            content_type: "application/json",
            cache_control: "public, max-age=60",
            include_content_length: true,
            meta_mode: MetaStoreMode::BestEffort,
            strip_transfer_encoding: false,
        },
        |headers| async move {
            simple_get(client, &upstream_url, &headers, Some("application/json")).await
        },
        move |body| async move {
            let mut metadata: JsonValue =
                serde_json::from_slice(&body).context("parsing npm metadata")?;
            if let Some((index, name)) = screened {
                delay::npm::filter_packument(delay, &index, &name, &mut metadata).await;
            }
            // Transform tarball URLs to point to our proxy
            transform_metadata(&mut metadata, &our_base);
            serde_json::to_vec(&metadata).context("serializing transformed metadata")
        },
    )
    .await?;

    Ok((result.response, result.outcome))
}

/// Handle tarball download request
///
/// Fetches from registry.npmjs.org with permanent caching.
async fn handle_tarball_download(
    client: &DefaultHttpWebClient,
    npm_req: &NpmPackageRequest,
    storage: Arc<FilesystemStorage>,
    index: Arc<CacheBackend>,
    delay: &DelayPolicyConfig,
    registry_base: &str,
) -> Result<(Response<Body>, CacheOutcome)> {
    let storage_path = npm_req.storage_path();
    let registry_base = registry_base.trim_end_matches('/').to_string();

    // URL-encode the package name for scoped packages
    let encoded_name = npm_req.name.replace('/', "%2f");
    let tarball_name = npm_req.tarball_name.as_deref().unwrap_or("package.tgz");
    let upstream_url = format!("{registry_base}/{encoded_name}/-/{tarball_name}");

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

    if delay::is_blocked(delay, &index, &cacheable.asset_key()).await {
        return respond_error(StatusCode::FORBIDDEN, BLOCKED);
    }

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

    let response = simple_get(client, &upstream_url, &HeaderMap::new(), None).await?;
    if !response.status().is_success() {
        let forwarded = buffer_response(response, true).await?;
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
    client: &DefaultHttpWebClient,
    req: Request<Body>,
    registry_base: &str,
) -> Result<(Response<Body>, CacheOutcome)> {
    let path = req.uri().path_or_root().into_owned();
    if path == "/-/ping" && req.method() == Method::GET {
        let response = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))?;
        return Ok((response, CacheOutcome::Pass));
    }

    let (parts, body) = req.into_parts();
    let path = parts.uri.path_or_root();
    let query = parts.uri.query_or_empty();
    let base = registry_base.trim_end_matches('/');
    let upstream_url = if query.is_empty() {
        format!("{base}{path}")
    } else {
        format!("{base}{path}?{query}")
    };

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

    let response = client
        .serve(request)
        .await
        .map_err(|e| anyhow::anyhow!("npm api request failed: {e}"))?;

    let forwarded = buffer_response(response, true).await?;
    Ok((forwarded, CacheOutcome::Pass))
}

/// Rewrites a `dist.tarball` URL to point at `our_base` instead of the upstream
/// npm registry.
fn rewrite_dist_tarball(dist: &mut serde_json::Map<String, JsonValue>, our_base: &str) {
    if let Some(tarball) = dist.get("tarball").and_then(|t| t.as_str()) {
        let new_tarball = tarball
            .replace("https://registry.npmjs.org", our_base)
            .replace("http://registry.npmjs.org", our_base);
        dist.insert("tarball".to_string(), JsonValue::String(new_tarball));
    }
}

/// Transform package metadata to point tarball URLs to our proxy
fn transform_metadata(metadata: &mut JsonValue, our_base: &str) {
    // Transform top-level dist.tarball (version-specific metadata)
    if let Some(dist) = metadata.get_mut("dist").and_then(|d| d.as_object_mut()) {
        rewrite_dist_tarball(dist, our_base);
    }

    // Transform dist.tarball URLs in all versions
    if let Some(versions) = metadata.get_mut("versions").and_then(|v| v.as_object_mut()) {
        for (_version, version_data) in versions.iter_mut() {
            if let Some(dist) = version_data.get_mut("dist").and_then(|d| d.as_object_mut()) {
                rewrite_dist_tarball(dist, our_base);
            }
        }
    }
}

fn copy_request_headers(source: &HeaderMap, target: &mut HeaderMap) {
    for (name, value) in source {
        if is_hop_header(name) || name == header::HOST || name == header::CONTENT_LENGTH {
            continue;
        }
        target.append(name, value.clone());
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
pub mod test_override {
    use std::sync::MutexGuard;
    use std::sync::{Mutex, OnceLock};

    static OVERRIDE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    static NPM_REGISTRY_BASE_OVERRIDE: OnceLock<Mutex<Option<String>>> = OnceLock::new();

    pub struct TestNpmRegistryBaseGuard {
        _lock: MutexGuard<'static, ()>,
    }

    impl Drop for TestNpmRegistryBaseGuard {
        fn drop(&mut self) {
            let slot = NPM_REGISTRY_BASE_OVERRIDE.get_or_init(|| Mutex::new(None));
            *slot.lock().unwrap() = None;
        }
    }

    pub fn override_npm_registry_base(base: impl Into<String>) -> TestNpmRegistryBaseGuard {
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
mod tests;
