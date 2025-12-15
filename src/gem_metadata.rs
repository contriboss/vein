use anyhow::Result;
use std::path::Path;
use tokio::task;
use vein_adapter::GemMetadata;

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
    let platform = platform.map(|p| p.to_owned());
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
