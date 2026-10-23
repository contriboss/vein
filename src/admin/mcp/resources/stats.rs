use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceRouter;
use serde::Serialize;

use crate::admin::mcp::{VeinMcp, json_resource, read_error};

const URI: &str = "vein://stats";

#[derive(Serialize)]
struct Stats {
    /// Upstream `RubyGems` source, if one is configured.
    upstream: Option<String>,
    /// Gem names known to the rubygems.org catalog.
    catalog_gems: u64,
    cached_assets: u64,
    rubygems_assets: u64,
    crate_assets: u64,
    npm_assets: u64,
    unique_packages: u64,
    total_size_bytes: u64,
    last_accessed: Option<String>,
    /// Cached gem versions whose gemspec was analysed, and how many of those
    /// have a `CycloneDX` SBOM.
    analysed_gem_versions: u64,
    gem_versions_with_sbom: u64,
}

impl VeinMcp {
    /// Cache summary: asset counts per ecosystem, size, catalog size, and
    /// SBOM coverage.
    #[mcp_resource(uri = "vein://stats", name = "stats", mime_type = "application/json")]
    async fn stats(&self, _ctx: Ctx<'_>) -> ResourceResult {
        let snapshot = self.resources.snapshot().await.map_err(read_error)?;
        json_resource(
            URI,
            &Stats {
                upstream: snapshot.upstream,
                catalog_gems: snapshot.catalog_total,
                cached_assets: snapshot.index.total_assets,
                rubygems_assets: snapshot.index.rubygems_assets,
                crate_assets: snapshot.index.crate_assets,
                npm_assets: snapshot.index.npm_assets,
                unique_packages: snapshot.index.unique_packages,
                total_size_bytes: snapshot.index.total_size_bytes,
                last_accessed: snapshot.index.last_accessed,
                analysed_gem_versions: snapshot.sbom.metadata_rows,
                gem_versions_with_sbom: snapshot.sbom.with_sbom,
            },
        )
    }
}

pub fn mount(router: McpResourceRouter<VeinMcp>) -> Result<McpResourceRouter<VeinMcp>, AppsError> {
    Ok(router.with_resource(
        VeinMcp::stats_resource_info()?,
        VeinMcp::stats_handler,
        None,
    ))
}
