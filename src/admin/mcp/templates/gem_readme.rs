use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use vein_adapter::AssetKind;

use crate::admin::mcp::VeinMcp;

impl VeinMcp {
    /// A gem version's README, from its `.gem` (fetched through the proxy when not cached), as its authors
    /// wrote it.
    #[mcp_resource_template(
        uri_template = "vein://gems/{name}/versions/{version}/readme",
        name = "gem_readme",
        title = "Gem README",
        mime_type = "application/json"
    )]
    async fn gem_readme(&self, ctx: Ctx<'_>) -> ResourceResult {
        self.readme(&ctx, AssetKind::Gem, "vein://gems").await
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::gem_readme_template_info()?,
        VeinMcp::gem_readme_handler,
        None,
    ))
}
