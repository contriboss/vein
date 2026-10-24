use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use vein_adapter::AssetKind;

use crate::admin::mcp::VeinMcp;

impl VeinMcp {
    /// One module or item of a crate version's API, from docs.rs: its
    /// signature, doc comment, and members (`runtime::Runner`).
    #[mcp_resource_template(
        uri_template = "vein://crates/{name}/versions/{version}/api/{+path}",
        name = "crate_api_item",
        title = "Crate API item",
        mime_type = "application/json"
    )]
    async fn crate_api_item(&self, ctx: Ctx<'_>) -> ResourceResult {
        self.api(&ctx, AssetKind::Crate, "vein://crates").await
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::crate_api_item_template_info()?,
        VeinMcp::crate_api_item_handler,
        None,
    ))
}
