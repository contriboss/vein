use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use vein_adapter::AssetKind;

use crate::admin::mcp::VeinMcp;

impl VeinMcp {
    /// An npm package version's README, from its tarball (fetched through the proxy when not cached), as its authors
    /// wrote it.
    #[mcp_resource_template(
        uri_template = "vein://npm/{+name}/versions/{version}/readme",
        name = "npm_readme",
        title = "npm package README",
        mime_type = "application/json"
    )]
    async fn npm_readme(&self, ctx: Ctx<'_>) -> ResourceResult {
        self.readme(&ctx, AssetKind::NpmPackage, "vein://npm").await
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::npm_readme_template_info()?,
        VeinMcp::npm_readme_handler,
        None,
    ))
}
