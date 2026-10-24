use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use vein_adapter::AssetKind;

use crate::admin::mcp::VeinMcp;

impl VeinMcp {
    /// A gem version's API from its Ruby source: its classes and modules,
    /// each with its `RDoc` summary; `.../api/{path}` for one of them.
    #[mcp_resource_template(
        uri_template = "vein://gems/{name}/versions/{version}/api",
        name = "gem_api",
        title = "Gem API",
        mime_type = "application/json"
    )]
    async fn gem_api(&self, ctx: Ctx<'_>) -> ResourceResult {
        self.api(&ctx, AssetKind::Gem, "vein://gems").await
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::gem_api_template_info()?,
        VeinMcp::gem_api_handler,
        None,
    ))
}
