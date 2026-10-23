//! Admin dashboard and the `/mcp` endpoint, served on their own listener
//! next to the proxy (`[admin]` in `vein.toml`).

pub mod app;
mod controllers;
pub mod mcp;
mod router;
pub mod ruby;
pub mod state;
pub mod trust;
mod utils;
mod views;

use std::sync::Arc;

use anyhow::Result;
use rama::http::service::web::Router;
use vein_adapter::{CacheBackend, FilesystemStorage};

use crate::config::Config;
use state::AdminState;

/// The admin HTTP router over the proxy's config, cache, and storage, with
/// `/mcp` mounted when `config.admin.mcp` allows it. Must run inside a Tokio
/// runtime.
pub async fn router(
    config: Arc<Config>,
    cache: Arc<CacheBackend>,
    storage: Arc<FilesystemStorage>,
) -> Result<Router<AdminState>> {
    let state = app::bootstrap(config.clone(), cache, storage).await?;
    let admin = &config.admin;
    let networks = trust::trusted_networks(admin)?;
    let mcp = mcp::endpoint(&admin.mcp, &admin.host, networks, state.resources.clone())?;
    if mcp.is_some() {
        tracing::info!(url = %format!("http://{}/mcp", admin.bind_addr()), "MCP endpoint enabled");
    }
    Ok(router::build(state, mcp))
}
