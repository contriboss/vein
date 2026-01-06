use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::info;
use vein::{config::Config as VeinConfig, db};
use vein_adapter::CacheBackend;

pub mod index;
pub mod sync;
pub mod validate;

/// Shared application context for CLI commands.
pub struct AppContext {
    pub config: Arc<VeinConfig>,
    pub cache: Arc<CacheBackend>,
}

impl AppContext {
    /// Initialize logging, load config, and connect to cache backend.
    pub async fn init() -> Result<Self> {
        tracing_subscriber::fmt()
            .with_target(false)
            .with_level(true)
            .init();

        info!("Loading Vein configuration...");

        let config_path = std::env::var("VEIN_CONFIG_PATH")
            .ok()
            .map(std::path::PathBuf::from);
        let config = Arc::new(VeinConfig::load(config_path).context("loading config")?);

        let (cache, _backend) = db::connect_cache_backend(&config)
            .await
            .context("connecting to cache backend")?;

        Ok(Self { config, cache })
    }
}
