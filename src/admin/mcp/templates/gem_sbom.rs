use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;

use crate::admin::mcp::views::gem_sbom_uri;
use crate::admin::mcp::{VeinMcp, json_resource_as, uri_param};

const CYCLONEDX_JSON: &str = "application/vnd.cyclonedx+json";

impl VeinMcp {
    /// `CycloneDX` SBOM Vein generated for a cached version.
    #[mcp_resource_template(
        uri_template = "vein://gems/{name}/versions/{version}/sbom",
        name = "gem_sbom",
        title = "Gem SBOM",
        mime_type = "application/vnd.cyclonedx+json"
    )]
    async fn gem_sbom(&self, ctx: Ctx<'_>) -> ResourceResult {
        let name = uri_param(&ctx, "name")?;
        let version = uri_param(&ctx, "version")?;
        let meta = self.cached_metadata(name, version).await?;
        let uri = gem_sbom_uri(&meta.name, &meta.version, &meta.platform);
        let Some(sbom) = meta.sbom else {
            return Err(ResourceError::NotFound(format!(
                "no SBOM recorded for {name} {}",
                meta.version
            )));
        };
        json_resource_as(uri, &sbom, CYCLONEDX_JSON)
    }
}

pub fn mount(router: McpResourceTemplateRouter<VeinMcp>) -> McpResourceTemplateRouter<VeinMcp> {
    router.with_template(
        VeinMcp::gem_sbom_template_info(),
        VeinMcp::gem_sbom_handler,
        None,
    )
}
