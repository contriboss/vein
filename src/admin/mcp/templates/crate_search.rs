use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use vein_adapter::AssetKind;

use crate::admin::mcp::VeinMcp;

impl VeinMcp {
    /// Search every crate crates.io publishes: its own search, or vein's
    /// name catalog when crates.io is out of reach.
    #[mcp_resource_template(
        uri_template = "vein://search/crates/{query}",
        name = "crate_search",
        title = "Search crates",
        mime_type = "application/json"
    )]
    async fn crate_search(&self, ctx: Ctx<'_>) -> ResourceResult {
        self.registry_search(&ctx, AssetKind::Crate, "crates", "crates.io")
            .await
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::crate_search_template_info()?,
        VeinMcp::crate_search_handler,
        None,
    ))
}
