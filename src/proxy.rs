pub(crate) mod cache;
#[cfg(feature = "gems")]
mod compact;
mod dispatch;
mod fetch;
mod handlers;
mod public_base;
mod response;
pub mod route;
#[cfg(feature = "gems")]
mod sbom;
#[cfg(test)]
mod tests;
pub(crate) mod types;
mod utils;

use std::sync::Arc;

use anyhow::{Context, Result};
use rama::{http::client::DefaultHttpWebClient, telemetry::tracing::info};

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
    /// Pooled client for the fixed npm and crates.io registries.
    http: DefaultHttpWebClient,
    /// Where package searches go before vein's catalog answers them.
    search: crate::search::Registries,
    /// Database pool for storing package metadata (exports, etc.)
    pub pool: Arc<sqlx::PgPool>,
    /// Stampeded job queue for background extraction tasks
    #[cfg(feature = "crates")]
    pub queue: Arc<stampeded::Queue>,
}

impl VeinProxy {
    pub fn new(
        config: Arc<Config>,
        storage: Arc<FilesystemStorage>,
        index: Arc<CacheBackend>,
        pool: Arc<sqlx::PgPool>,
        #[cfg(feature = "crates")] queue: Arc<stampeded::Queue>,
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

        // Gem searches go where gems come from: rubygems.org or a private server.
        let search = crate::search::Registries {
            gems: config.upstream.as_ref().map_or_else(
                || crate::search::Registries::default().gems,
                |upstream| upstream.url.to_string().trim_end_matches('/').to_owned(),
            ),
            ..crate::search::Registries::default()
        };

        Ok(Self {
            config,
            storage,
            index,
            upstreams,
            upstream_client,
            http: DefaultHttpWebClient::default(),
            search,
            pool,
            #[cfg(feature = "crates")]
            queue,
        })
    }
}
