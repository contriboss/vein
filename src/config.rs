use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

// Re-export all submodules
pub mod database;
pub mod delay_policy;
pub mod logging;
pub mod reliability;
pub mod server;
pub mod storage;
pub mod upstream;

#[cfg(test)]
mod tests;

// Re-export types from submodules for convenience
pub use database::{DatabaseBackend, DatabaseConfig};
pub use delay_policy::{DelayPolicyConfig, GemDelayOverride, PinnedVersion};
pub use logging::LoggingConfig;
pub use reliability::{BackoffStrategy, ReliabilityConfig, RetryConfig};
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
}

impl Config {
    pub fn load(path: Option<PathBuf>) -> Result<Self> {
        let candidate = path.unwrap_or_else(|| PathBuf::from("vein.toml"));
        if candidate.exists() {
            let raw = fs::read_to_string(&candidate)
                .with_context(|| format!("failed to read config {}", candidate.display()))?;
            let mut config: Config = toml::from_str(&raw)
                .with_context(|| format!("invalid config {}", candidate.display()))?;
            config
                .storage
                .normalize_paths(candidate.parent().unwrap_or(Path::new(".")));
            config
                .database
                .normalize_paths(candidate.parent().unwrap_or(Path::new(".")));
            Ok(config)
        } else {
            if let Some(path) = candidate.to_str() {
                tracing::warn!("configuration file {path} not found, using defaults");
            } else {
                tracing::warn!("configuration file not found, using defaults");
            }
            let mut config = Config::default();
            let cwd = std::env::current_dir().context("reading current directory")?;
            config.storage.normalize_paths(&cwd);
            config.database.normalize_paths(&cwd);
            Ok(config)
        }
    }

    pub fn validate(&self) -> Result<()> {
        if let Some(upstream) = self
            .upstream
            .as_ref()
            .filter(|upstream| upstream.url.scheme() != "https" && upstream.url.scheme() != "http")
        {
            bail!("unsupported upstream scheme {}", upstream.url);
        }
        self.database.backend()?;
        Ok(())
    }
}
