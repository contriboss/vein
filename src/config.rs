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

    pub fn validate(&self) -> Result<()> {
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
