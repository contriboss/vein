use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;

use vein_adapter::AssetKind;

use crate::admin::mcp::advisories::for_version;
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
        let meta = self.cached_metadata(name, version).await?;
        let uri = gem_version_uri(&meta.name, &meta.version, &meta.platform);
        let advisories = for_version(
            AssetKind::Gem,
            &self.advisories(AssetKind::Gem, &meta.name).await,
            &meta.version,
        );
        json_resource(
            uri,
            &VersionDetails {
                advisories,
                ..VersionDetails::from(meta)
            },
        )
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::gem_version_template_info()?,
        VeinMcp::gem_version_handler,
        None,
    ))
}
