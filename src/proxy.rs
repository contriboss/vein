pub(crate) mod cache;
mod compact;
mod dispatch;
mod fetch;
mod handlers;
mod quarantine;
mod response;
#[cfg(test)]
mod tests;
pub(crate) mod types;
mod utils;

use std::sync::Arc;

use anyhow::{Context, Result};
use rama::telemetry::tracing::info;

use crate::{config::Config, upstream::UpstreamClient};
use vein_adapter::{CacheBackend, FilesystemStorage};

pub use types::{CacheStatus, RequestContext, UpstreamTarget};

/// Main proxy service.
#[derive(Clone)]
pub struct VeinProxy {
    config: Arc<Config>,
    storage: Arc<FilesystemStorage>,
    index: Arc<CacheBackend>,
    upstreams: Vec<UpstreamTarget>,
    upstream_client: Option<UpstreamClient>,
}

impl VeinProxy {
    pub fn new(
        config: Arc<Config>,
        storage: Arc<FilesystemStorage>,
        index: Arc<CacheBackend>,
    ) -> Result<Self> {
        let (upstreams, upstream_client) = if let Some(ref upstream_config) = config.upstream {
            let mut upstreams = Vec::new();
            upstreams.push(UpstreamTarget::from_url(&upstream_config.url)?);
            for url in &upstream_config.fallback_urls {
                upstreams.push(UpstreamTarget::from_url(url)?);
            }
            let client =
                UpstreamClient::new(upstream_config).context("building upstream client")?;
            (upstreams, Some(client))
        } else {
            info!("No upstream configured - running in cache-only mode");
            (Vec::new(), None)
        };

        Ok(Self {
            config,
            storage,
            index,
            upstreams,
            upstream_client,
        })
    }
}
