use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use vein_adapter::AssetKind;

use crate::admin::mcp::VeinMcp;

impl VeinMcp {
    /// A crate version's API from docs.rs: its outline (modules and items by
    /// kind, each with its summary); `.../api/{path}` for one of them.
    #[mcp_resource_template(
        uri_template = "vein://crates/{name}/versions/{version}/api",
        name = "crate_api",
        title = "Crate API",
        mime_type = "application/json"
    )]
    async fn crate_api(&self, ctx: Ctx<'_>) -> ResourceResult {
        self.api(&ctx, AssetKind::Crate, "vein://crates").await
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::crate_api_template_info()?,
        VeinMcp::crate_api_handler,
        None,
    ))
}
