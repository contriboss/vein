use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use vein_adapter::AssetKind;

use crate::admin::mcp::VeinMcp;

impl VeinMcp {
    /// A cached gem version's classes, modules, methods and constants, as the
    /// exports index read them.
    #[mcp_resource_template(
        uri_template = "vein://gems/{name}/versions/{version}/exports",
        name = "gem_exports",
        title = "Gem exports",
        mime_type = "application/json"
    )]
    async fn gem_exports(&self, ctx: Ctx<'_>) -> ResourceResult {
        self.exports(&ctx, AssetKind::Gem, "vein://gems").await
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::gem_exports_template_info()?,
        VeinMcp::gem_exports_handler,
        None,
    ))
}
