use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;

use crate::admin::mcp::VeinMcp;

impl VeinMcp {
    /// Cached package versions exporting a symbol, across registries: exact
    /// names first, then case-insensitive, prefix and containing ones.
    #[mcp_resource_template(
        uri_template = "vein://search/exports/{query}",
        name = "exports_search",
        title = "Search exported symbols",
        mime_type = "application/json"
    )]
    async fn exports_search_resource(&self, ctx: Ctx<'_>) -> ResourceResult {
        self.exports_search(&ctx).await
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::exports_search_resource_template_info()?,
        VeinMcp::exports_search_resource_handler,
        None,
    ))
}
