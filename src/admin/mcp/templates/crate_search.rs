use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use vein_adapter::AssetKind;

use crate::admin::mcp::VeinMcp;

impl VeinMcp {
    /// Search the crates this proxy has cached by substring. There is no
    /// crates.io-wide catalog here.
    #[mcp_resource_template(
        uri_template = "vein://crates/search/{query}",
        name = "crate_search",
        title = "Search crates",
        mime_type = "application/json"
    )]
    async fn crate_search(&self, ctx: Ctx<'_>) -> ResourceResult {
        self.cached_search(&ctx, AssetKind::Crate, "crates").await
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
