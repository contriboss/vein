use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use vein_adapter::AssetKind;

use crate::admin::mcp::VeinMcp;

impl VeinMcp {
    /// A cached crate version's public items from its `lib.rs`, as the exports
    /// index read them.
    #[mcp_resource_template(
        uri_template = "vein://crates/{name}/versions/{version}/exports",
        name = "crate_exports",
        title = "Crate exports",
        mime_type = "application/json"
    )]
    async fn crate_exports(&self, ctx: Ctx<'_>) -> ResourceResult {
        self.exports(&ctx, AssetKind::Crate, "vein://crates").await
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::crate_exports_template_info()?,
        VeinMcp::crate_exports_handler,
        None,
    ))
}
