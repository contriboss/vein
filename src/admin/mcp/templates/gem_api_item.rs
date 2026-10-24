use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use vein_adapter::AssetKind;

use crate::admin::mcp::VeinMcp;

impl VeinMcp {
    /// One class, module or method of a gem version's API, from its Ruby
    /// source: its `RDoc` comments, mixins, attributes and methods
    /// (`StateMachines::Machine`, `StateMachines::Machine#draw`).
    #[mcp_resource_template(
        uri_template = "vein://gems/{name}/versions/{version}/api/{+path}",
        name = "gem_api_item",
        title = "Gem API item",
        mime_type = "application/json"
    )]
    async fn gem_api_item(&self, ctx: Ctx<'_>) -> ResourceResult {
        self.api(&ctx, AssetKind::Gem, "vein://gems").await
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::gem_api_item_template_info()?,
        VeinMcp::gem_api_item_handler,
        None,
    ))
}
