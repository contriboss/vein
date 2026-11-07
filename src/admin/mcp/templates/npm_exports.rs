use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use vein_adapter::AssetKind;

use crate::admin::mcp::VeinMcp;

impl VeinMcp {
    /// A cached npm version's exported declarations, as the exports index read
    /// them from its TypeScript declarations.
    #[mcp_resource_template(
        uri_template = "vein://npm/{+name}/versions/{version}/exports",
        name = "npm_exports",
        title = "npm package exports",
        mime_type = "application/json"
    )]
    async fn npm_exports(&self, ctx: Ctx<'_>) -> ResourceResult {
        self.exports(&ctx, AssetKind::NpmPackage, "vein://npm")
            .await
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::npm_exports_template_info()?,
        VeinMcp::npm_exports_handler,
        None,
    ))
}
