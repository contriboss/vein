use anyhow::Result;
use rama::telemetry::tracing::{debug, warn};
use std::path::Path;
use tokio::task;
use vein_adapter::{AssetKey, CacheBackend, CacheBackendTrait, GemMetadata};

mod analyzer;
mod parser;
mod sbom;

#[cfg(test)]
mod tests;

pub use parser::parse_gem_metadata;

/// Extract structured metadata from a cached gem archive.
///
/// Returns `Ok(None)` when the gem does not contain a metadata payload we understand.
pub async fn extract_gem_metadata(
    path: &Path,
    name: &str,
    version: &str,
    platform: Option<&str>,
    size_bytes: u64,
    sha256: &str,
    existing_sbom: Option<serde_json::Value>,
) -> Result<Option<GemMetadata>> {
    let path = path.to_owned();
    let name = name.to_owned();
    let version = version.to_owned();
    let platform = platform.map(std::borrow::ToOwned::to_owned);
    let sha256 = sha256.to_owned();

    task::spawn_blocking(move || {
        parse_gem_metadata(
            &path,
            &name,
            &version,
            platform,
            size_bytes,
            &sha256,
            existing_sbom,
        )
    })
    .await?
}

/// Records the metadata (and SBOM) of gem `key`, just cached at `path`.
/// Failures are logged: the gem is served either way.
pub async fn record_cached(
    index: &CacheBackend,
    path: &Path,
    key: &AssetKey<'_>,
    size_bytes: u64,
    sha256: &str,
) {
    let existing_sbom = match index
        .gem_metadata(key.name, key.version, key.platform)
        .await
    {
        Ok(meta) => meta.and_then(|meta| meta.sbom),
        Err(err) => {
            warn!(
                error = %err,
                "failed to look up existing metadata while preparing SBOM"
            );
            None
        }
    };
    match extract_gem_metadata(
        path,
        key.name,
        key.version,
        key.platform,
        size_bytes,
        sha256,
        existing_sbom,
    )
    .await
    {
        Ok(Some(metadata)) => {
            if let Err(err) = index.upsert_metadata(&metadata).await {
                warn!(
                    error = %err,
                    path = %path.display(),
                    "failed to persist gem metadata"
                );
            }
        }
        Ok(None) => debug!(path = %path.display(), "gem metadata unavailable"),
        Err(err) => warn!(
            error = %err,
            path = %path.display(),
            "failed to analyze gem metadata"
        ),
    }
}
