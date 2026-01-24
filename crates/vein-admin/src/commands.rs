use std::{path::Path, sync::Arc};

use anyhow::{Context, Result};
use tracing::info;
use vein::{config::Config as VeinConfig, db};
use vein_adapter::CacheBackend;

pub mod sync;

/// Shared application context for CLI commands.
pub struct AppContext {
    pub config: Arc<VeinConfig>,
    pub cache: Arc<CacheBackend>,
}

impl AppContext {
    /// Load config and connect to the cache backend.
    pub async fn load(config_path: impl AsRef<Path>) -> Result<Self> {
        info!(path = %config_path.as_ref().display(), "Loading Vein configuration");

        let config = Arc::new(
            VeinConfig::load(Some(config_path.as_ref().to_path_buf())).context("loading config")?,
        );
        config.validate().context("validating config")?;

        let (cache, _backend) = db::connect_cache_backend(&config)
            .await
            .context("connecting to cache backend")?;

        Ok(Self { config, cache })
    }
}
