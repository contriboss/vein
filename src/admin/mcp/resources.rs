//! Fixed-URI resources, one per file.

mod quarantine;
mod stats;

use mcp_host::prelude::AppsError;
use mcp_host::registry::router::McpResourceRouter;

use super::VeinMcp;

pub fn router() -> Result<McpResourceRouter<VeinMcp>, AppsError> {
    let router = McpResourceRouter::new();
    let router = stats::mount(router)?;
    quarantine::mount(router)
}
