use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use rama::http::{Body, Response, StatusCode, body::util::BodyExt, header};
use rama::telemetry::tracing::{debug, warn};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use vein_adapter::{AssetKind, CacheBackend, CacheBackendTrait, CachedAsset, FilesystemStorage, TempFile};

use super::types::CacheableRequest;

/// Serves a cached file to the client
pub async fn serve_cached(
    cacheable: &CacheableRequest,
    entry: CachedAsset,
    storage: &FilesystemStorage,
) -> Result<Response<Body>> {
    let path = storage.resolve(&entry.path);
    let data = tokio::fs::read(&path)
        .await
        .with_context(|| format!("reading cached file {}", path.display()))?;

    build_cached_response(cacheable, entry.sha256, data)
}

fn build_cached_response(
    cacheable: &CacheableRequest,
    sha256: String,
    data: Vec<u8>,
) -> Result<Response<Body>> {
    let mut builder = Response::builder().status(StatusCode::OK);
    {
        let headers = builder
            .headers_mut()
            .ok_or_else(|| anyhow!("failed to get headers for cached response"))?;
        headers.insert(
            header::CONTENT_LENGTH,
            header::HeaderValue::from_str(&data.len().to_string())?,
        );
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static(cacheable.content_type()),
        );
        headers.insert(
            header::HeaderName::from_static("x-checksum-sha256"),
            header::HeaderValue::from_str(&sha256)?,
        );
        headers.insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("public, max-age=31536000"),
        );
        headers.insert(
            header::CONTENT_DISPOSITION,
            header::HeaderValue::from_str(&format!(
                "attachment; filename=\"{}\"",
                cacheable.download_name()
            ))?,
        );
    }

    builder.body(Body::from(data)).map_err(Into::into)
}

/// Runs the cache miss flow: fetch body, persist to cache, return response
pub async fn run_cache_miss_flow(
    cacheable: &CacheableRequest,
    index: Arc<CacheBackend>,
    storage: Arc<FilesystemStorage>,
    response: rama::http::Response<rama::http::Body>,
    mut temp_file: TempFile,
    _treating_as_revalidation: bool,
) -> Result<Response<Body>> {
    let status = response.status();
    let headers = response.headers().clone();
    let body_bytes = response
        .into_body()
        .collect()
        .await
        .context("reading upstream response body")?
        .to_bytes();

    // Persist to temp file first
    temp_file
        .file_mut()
        .write_all(&body_bytes)
        .await
        .context("writing body to temp file")?;
    temp_file.commit().await.context("committing temp file")?;

    // Compute checksum
    let sha_hex = {
        let mut hasher = Sha256::new();
        hasher.update(&body_bytes);
        hex::encode(hasher.finalize())
    };

    // Store metadata
    index
        .insert_or_replace(
            &cacheable.asset_key(),
            &cacheable.relative_path,
            &sha_hex,
            body_bytes.len() as u64,
        )
        .await
        .context("failed to store metadata for cached asset")?;

    if cacheable.kind == AssetKind::Gem {
        let absolute_path = storage.resolve(&cacheable.relative_path);
        let existing_sbom = match index
            .gem_metadata(
                &cacheable.name,
                &cacheable.version,
                cacheable.platform.as_deref(),
            )
            .await
        {
            Ok(Some(meta)) => meta.sbom,
            Ok(None) => None,
            Err(err) => {
                warn!(
                    error = %err,
                    "failed to look up existing metadata while preparing SBOM"
                );
                None
            }
        };
        match crate::gem_metadata::extract_gem_metadata(
            &absolute_path,
            &cacheable.name,
            &cacheable.version,
            cacheable.platform.as_deref(),
            body_bytes.len() as u64,
            &sha_hex,
            existing_sbom,
        )
        .await
        {
            Ok(Some(metadata)) => {
                if let Err(err) = index.upsert_metadata(&metadata).await {
                    warn!(
                        error = %err,
                        path = %absolute_path.display(),
                        "failed to persist gem metadata"
                    );
                }
            }
            Ok(None) => {
                debug!(path = %absolute_path.display(), "gem metadata unavailable");
            }
            Err(err) => {
                warn!(
                    error = %err,
                    path = %absolute_path.display(),
                    "failed to analyze gem metadata"
                );
            }
        }
    }

    // Build client response using upstream headers with our cache headers
    let mut builder = Response::builder().status(status);
    {
        let hdrs = builder
            .headers_mut()
            .ok_or_else(|| anyhow!("failed to get headers for cache miss response"))?;

        copy_header_if_present(&headers, hdrs, header::CONTENT_LENGTH)?;
        copy_header_if_present(&headers, hdrs, header::CONTENT_TYPE)?;
        copy_header_if_present(&headers, hdrs, header::LAST_MODIFIED)?;
        copy_header_if_present(&headers, hdrs, header::ETAG)?;

        if !hdrs.contains_key(header::CONTENT_TYPE) {
            hdrs.insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static(cacheable.content_type()),
            );
        }
        hdrs.insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("public, max-age=31536000"),
        );
        hdrs.insert(
            header::CONTENT_DISPOSITION,
            header::HeaderValue::from_str(&format!(
                "attachment; filename=\"{}\"",
                cacheable.download_name()
            ))?,
        );
    }

    builder.body(Body::from(body_bytes)).map_err(Into::into)
}

fn copy_header_if_present(
    source: &header::HeaderMap,
    target: &mut header::HeaderMap,
    name: header::HeaderName,
) -> Result<()> {
    if let Some(value) = source.get(&name) {
        target.insert(name, value.clone());
    }
    Ok(())
}
