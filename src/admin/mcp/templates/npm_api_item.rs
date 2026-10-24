use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use vein_adapter::AssetKind;

use crate::admin::mcp::VeinMcp;

impl VeinMcp {
    /// One declaration file of an npm package version, by its path in the package (`dist/index.d.ts`).
    #[mcp_resource_template(
        uri_template = "vein://npm/{+name}/versions/{version}/api/{+path}",
        name = "npm_api_item",
        title = "npm package API file",
        mime_type = "application/json"
    )]
    async fn npm_api_item(&self, ctx: Ctx<'_>) -> ResourceResult {
        self.api(&ctx, AssetKind::NpmPackage, "vein://npm").await
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::npm_api_item_template_info()?,
        VeinMcp::npm_api_item_handler,
        None,
    ))
}
