//! The exports index: the public API of every cached gem, crate and npm
//! version, read in the background into `package_exports`, so `/mcp` can
//! say which package exports a symbol without fetching any of them.
//!
//! A recurring job reads what the cache holds and the index has not: new
//! downloads within minutes, everything cached before on its first runs.

use anyhow::{Context, Result};
use rama::telemetry::tracing::warn;
use vein_adapter::{
    AssetKind, CacheBackend, CacheBackendTrait, FilesystemStorage, PackageExport, PendingExports,
};

/// The stampeded task that indexes pending versions.
pub const TASK: &str = "exports.index";
/// Versions fetched from the ledger per round.
const BATCH: i64 = 50;

/// Indexes every cached version not indexed yet, until none is left or
/// `stop` says so; how many were read. A version that cannot be read is
/// recorded with its error and not retried.
pub async fn index_pending(
    index: &CacheBackend,
    storage: &FilesystemStorage,
    stop: &(dyn Fn() -> bool + Sync),
) -> Result<usize> {
    let kinds = crate::registry::served();
    let mut indexed = 0;
    loop {
        let pending = index.exports_pending(&kinds, BATCH).await?;
        if pending.is_empty() {
            return Ok(indexed);
        }
        for version in pending {
            if stop() {
                return Ok(indexed);
            }
            match read(storage, &version).await {
                Ok(exports) => {
                    index
                        .exports_replace(version.kind, &version.name, &version.version, &exports)
                        .await?;
                }
                Err(err) => {
                    let error = format!("{err:#}");
                    warn!(
                        registry = version.kind.as_str(),
                        name = %version.name,
                        version = %version.version,
                        %error,
                        "exports: could not read a cached version"
                    );
                    index
                        .exports_failed(version.kind, &version.name, &version.version, &error)
                        .await?;
                }
            }
            indexed += 1;
        }
    }
}

/// The exports of one cached archive, parsed off the async runtime.
async fn read(storage: &FilesystemStorage, version: &PendingExports) -> Result<Vec<PackageExport>> {
    let path = storage.resolve(&version.path);
    let bytes = tokio::fs::read(&path)
        .await
        .with_context(|| format!("reading {}", path.display()))?;
    let (kind, name, number) = (version.kind, version.name.clone(), version.version.clone());
    tokio::task::spawn_blocking(move || extract(kind, &name, &number, &bytes))
        .await
        .context("reading exports")?
}

// Without crates, `version` goes unused (only a `.crate` needs it to find
// its `lib.rs`), and so does `name` when npm is left out too.
#[cfg_attr(not(feature = "crates"), allow(unused_variables))]
fn extract(
    kind: AssetKind,
    name: &str,
    version: &str,
    archive: &[u8],
) -> Result<Vec<PackageExport>> {
    match kind {
        #[cfg(feature = "crates")]
        AssetKind::Crate => crate::crates::exports::from_crate(archive, name, version),
        #[cfg(feature = "gems")]
        AssetKind::Gem => {
            let sources = crate::docs::rdoc::sources(archive)?;
            Ok(crate::docs::rdoc::Api::parse(&sources).exports())
        }
        #[cfg(feature = "npm")]
        AssetKind::NpmPackage => {
            let typings = crate::docs::typings::read(archive)?;
            Ok(crate::docs::typings::exports(&typings, name))
        }
        #[allow(unreachable_patterns)]
        kind => Err(crate::registry::NotBuilt(kind).into()),
    }
}

#[cfg(test)]
mod tests;
