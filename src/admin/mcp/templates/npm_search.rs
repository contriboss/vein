use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use vein_adapter::AssetKind;

use crate::admin::mcp::VeinMcp;

impl VeinMcp {
    /// Search every package npm publishes: npm's own search, or vein's name
    /// catalog when npm is out of reach.
    #[mcp_resource_template(
        uri_template = "vein://search/npm/{query}",
        name = "npm_search",
        title = "Search npm packages",
        mime_type = "application/json"
    )]
    async fn npm_search(&self, ctx: Ctx<'_>) -> ResourceResult {
        self.registry_search(&ctx, AssetKind::NpmPackage, "npm", "npm")
            .await
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::npm_search_template_info()?,
        VeinMcp::npm_search_handler,
        None,
    ))
}
