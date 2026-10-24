use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use rama::net::Protocol;
use serde::Deserialize;
use vein_adapter::AssetKind;

// Re-export all submodules
pub mod admin;
pub mod catalog;
pub mod database;
pub mod delay_policy;
pub mod docs;
pub mod logging;
pub mod reliability;
pub mod server;
pub mod storage;
pub mod upstream;

#[cfg(test)]
mod tests;

/// Resolves a relative path against `base_dir`, leaving absolute paths intact.
pub(crate) fn resolve_relative(path: &mut PathBuf, base_dir: &Path) {
    if path.is_relative() {
        *path = base_dir.join(&*path);
    }
}

// Re-export types from submodules for convenience
pub use admin::{AdminConfig, McpSettings};
pub use catalog::CatalogConfig;
pub use database::{DatabaseBackend, DatabaseConfig};
pub use delay_policy::DelayPolicyConfig;
pub use docs::DocsConfig;
pub use logging::LoggingConfig;
pub use reliability::{BackoffStrategy, RetryConfig};
pub use server::ServerConfig;
pub use storage::StorageConfig;
pub use upstream::UpstreamConfig;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub upstream: Option<UpstreamConfig>,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub delay_policy: DelayPolicyConfig,
    #[serde(default)]
    pub admin: AdminConfig,
    #[serde(default)]
    pub catalog: CatalogConfig,
    #[serde(default)]
    pub docs: DocsConfig,
}

impl Config {
    pub fn load(path: Option<PathBuf>) -> Result<Self> {
        let candidate = path.unwrap_or_else(|| PathBuf::from("vein.toml"));
        if candidate.exists() {
            let raw = fs::read_to_string(&candidate)
                .with_context(|| format!("failed to read config {}", candidate.display()))?;
            let mut config: Self = toml::from_str(&raw)
                .with_context(|| format!("invalid config {}", candidate.display()))?;
            config
                .storage
                .normalize_paths(candidate.parent().unwrap_or_else(|| Path::new(".")));
            Ok(config)
        } else {
            if let Some(path) = candidate.to_str() {
                rama::telemetry::tracing::warn!(
                    "configuration file {path} not found, using defaults"
                );
            } else {
                rama::telemetry::tracing::warn!("configuration file not found, using defaults");
            }
            let mut config = Self::default();
            let cwd = std::env::current_dir().context("reading current directory")?;
            config.storage.normalize_paths(&cwd);
            Ok(config)
        }
    }

    /// Refuses settings for a registry this build left out: `[upstream]`
    /// is the gem source; catalogs, delays and pins name theirs.
    fn registries_built(&self) -> Result<()> {
        let upstream = self
            .upstream
            .is_some()
            .then_some((AssetKind::Gem, "upstream"));
        let named = upstream
            .into_iter()
            .chain(
                self.catalog
                    .registries
                    .iter()
                    .map(|&kind| (kind, "catalog.registries")),
            )
            .chain(
                self.delay_policy
                    .packages
                    .iter()
                    .filter_map(|entry| entry.registry)
                    .map(|kind| (kind, "delay_policy.packages")),
            )
            .chain(
                self.delay_policy
                    .pinned
                    .iter()
                    .filter_map(|pin| pin.registry)
                    .map(|kind| (kind, "delay_policy.pinned")),
            );
        for (kind, setting) in named {
            if !crate::registry::built(kind) {
                bail!(
                    "{setting} names {}, but {}",
                    kind.as_str(),
                    crate::registry::NotBuilt(kind)
                );
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        self.registries_built()?;
        if let Some(upstream) = self.upstream.as_ref().filter(|upstream| {
            upstream.url.scheme() != Some(&Protocol::HTTPS)
                && upstream.url.scheme() != Some(&Protocol::HTTP)
        }) {
            bail!("unsupported upstream scheme {}", upstream.url);
        }
        self.database.backend()?;
        if let Some(kind) = self.catalog.registries.iter().find(|kind| {
            !matches!(
                kind,
                AssetKind::Gem | AssetKind::Crate | AssetKind::NpmPackage
            )
        }) {
            bail!(
                "catalog.registries holds gem, crate or npm; got {}",
                kind.as_str()
            );
        }
        if let Some(index) = self
            .delay_policy
            .packages
            .iter()
            .position(|entry| entry.name.is_empty() && entry.names.is_empty())
        {
            bail!("delay_policy.packages[{index}] names no package: give `name` or `names`");
        }
        if self.delay_policy.release_hour_utc > 23 {
            bail!(
                "delay_policy.release_hour_utc is an hour of the day, 0 to 23; got {}",
                self.delay_policy.release_hour_utc
            );
        }
        Ok(())
    }
}
