use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use serde::Serialize;

use crate::admin::mcp::views::{TrackedVersion, VersionDetails, gem_version_uri};
use crate::admin::mcp::{DEFAULT_PLATFORM, VeinMcp, json_resource, read_error, uri_param};

#[derive(Serialize)]
struct Gem {
    name: String,
    /// Whether rubygems.org publishes this name, per the synced catalog.
    in_catalog: bool,
    /// URIs of the versions cached here, newest first.
    cached_versions: Vec<String>,
    /// Versions the release delay policy tracks, newest first.
    quarantine: Vec<TrackedVersion>,
    /// Metadata of the newest cached version (ruby platform), if analysed.
    latest: Option<VersionDetails>,
}

impl VeinMcp {
    /// A gem as seen by this proxy: cached versions, their quarantine status,
    /// and the newest version's gemspec metadata.
    #[mcp_resource_template(
        uri_template = "vein://gems/{name}",
        name = "gem",
        title = "Gem",
        mime_type = "application/json"
    )]
    async fn gem(&self, ctx: Ctx<'_>) -> ResourceResult {
        let name = uri_param(&ctx, "name")?;
        let versions = self
            .resources
            .gem_versions(name)
            .await
            .map_err(read_error)?;
        let quarantine = self
            .resources
            .tracked_versions(name)
            .await
            .map_err(read_error)?;
        let in_catalog = self
            .resources
            .catalog_search(name, 1)
            .await
            .map_err(read_error)?
            .first()
            .is_some_and(|found| found == name);
        let latest = match versions.first() {
            Some(version) => self
                .resources
                .gem_metadata(name, version, Some(DEFAULT_PLATFORM))
                .await
                .map_err(read_error)?
                .map(VersionDetails::from),
            None => None,
        };

        json_resource(
            format!("vein://gems/{name}"),
            &Gem {
                cached_versions: versions
                    .iter()
                    .map(|version| gem_version_uri(name, version, DEFAULT_PLATFORM))
                    .collect(),
                quarantine: quarantine.into_iter().map(TrackedVersion::from).collect(),
                in_catalog,
                latest,
                name: name.to_owned(),
            },
        )
    }
}

pub fn mount(router: McpResourceTemplateRouter<VeinMcp>) -> McpResourceTemplateRouter<VeinMcp> {
    router.with_template(VeinMcp::gem_template_info(), VeinMcp::gem_handler, None)
}
