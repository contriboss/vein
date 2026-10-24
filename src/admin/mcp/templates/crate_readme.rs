use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use vein_adapter::AssetKind;

use crate::admin::mcp::VeinMcp;

impl VeinMcp {
    /// A crate version's README, from its `.crate` (fetched through the proxy when not cached), as its authors
    /// wrote it.
    #[mcp_resource_template(
        uri_template = "vein://crates/{name}/versions/{version}/readme",
        name = "crate_readme",
        title = "Crate README",
        mime_type = "application/json"
    )]
    async fn crate_readme(&self, ctx: Ctx<'_>) -> ResourceResult {
        self.readme(&ctx, AssetKind::Crate, "vein://crates").await
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::crate_readme_template_info()?,
        VeinMcp::crate_readme_handler,
        None,
    ))
}
