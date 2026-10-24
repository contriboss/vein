use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use vein_adapter::AssetKind;

use crate::admin::mcp::VeinMcp;

impl VeinMcp {
    /// An npm package version's API as its TypeScript declarations (its own,
    /// else `DefinitelyTyped`'s): the main file, the others listed;
    /// `.../api/{file}` for one.
    #[mcp_resource_template(
        uri_template = "vein://npm/{+name}/versions/{version}/api",
        name = "npm_api",
        title = "npm package API",
        mime_type = "application/json"
    )]
    async fn npm_api(&self, ctx: Ctx<'_>) -> ResourceResult {
        self.api(&ctx, AssetKind::NpmPackage, "vein://npm").await
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::npm_api_template_info()?,
        VeinMcp::npm_api_handler,
        None,
    ))
}
