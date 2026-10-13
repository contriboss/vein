use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;

use crate::admin::mcp::views::{VersionDetails, gem_version_uri};
use crate::admin::mcp::{VeinMcp, json_resource, uri_param};

impl VeinMcp {
    /// Gemspec metadata of a cached version: summary, licenses, links,
    /// dependencies, native code. Use `1.16.0-x86_64-linux` for a platform build.
    #[mcp_resource_template(
        uri_template = "vein://gems/{name}/versions/{version}",
        name = "gem_version",
        title = "Gem version",
        mime_type = "application/json"
    )]
    async fn gem_version(&self, ctx: Ctx<'_>) -> ResourceResult {
        let name = uri_param(&ctx, "name")?;
        let version = uri_param(&ctx, "version")?;
        let details = VersionDetails::from(self.cached_metadata(name, version).await?);
        json_resource(
            gem_version_uri(&details.name, &details.version, &details.platform),
            &details,
        )
    }
}

pub fn mount(router: McpResourceTemplateRouter<VeinMcp>) -> McpResourceTemplateRouter<VeinMcp> {
    router.with_template(
        VeinMcp::gem_version_template_info(),
        VeinMcp::gem_version_handler,
        None,
    )
}
