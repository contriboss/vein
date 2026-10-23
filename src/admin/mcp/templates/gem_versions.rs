use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;

use crate::admin::mcp::rubygems::is_prerelease;
use crate::admin::mcp::version_list::VersionList;
use crate::admin::mcp::{VeinMcp, json_resource, unknown_package, uri_param};

impl VeinMcp {
    /// Every version rubygems.org publishes of a gem, from its compact index:
    /// releases and prereleases apart, newest first.
    #[mcp_resource_template(
        uri_template = "vein://gems/{name}/versions",
        name = "gem_versions",
        title = "Gem versions",
        mime_type = "application/json"
    )]
    async fn gem_versions(&self, ctx: Ctx<'_>) -> ResourceResult {
        let name = uri_param(&ctx, "name")?;
        let published = self.gem_info(name).await?;
        if published.is_empty() {
            return Err(unknown_package(format_args!("gem {name}")));
        }
        json_resource(
            format!("vein://gems/{name}/versions"),
            &VersionList::split(published.iter().map(String::as_str), is_prerelease),
        )
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::gem_versions_template_info()?,
        VeinMcp::gem_versions_handler,
        None,
    ))
}
