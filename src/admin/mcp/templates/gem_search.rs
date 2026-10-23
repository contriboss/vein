use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;

use vein_adapter::AssetKind;

use crate::admin::mcp::VeinMcp;

impl VeinMcp {
    /// Search every gem rubygems.org (or vein's configured upstream)
    /// publishes: its own search, or vein's name catalog when it is out of
    /// reach.
    #[mcp_resource_template(
        uri_template = "vein://search/gems/{query}",
        name = "gem_search",
        title = "Search gems",
        mime_type = "application/json"
    )]
    async fn gem_search(&self, ctx: Ctx<'_>) -> ResourceResult {
        self.registry_search(&ctx, AssetKind::Gem, "gems", "rubygems.org")
            .await
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::gem_search_template_info()?,
        VeinMcp::gem_search_handler,
        None,
    ))
}
