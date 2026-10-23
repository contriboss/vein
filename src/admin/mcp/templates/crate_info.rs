use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use serde::Serialize;
use vein_adapter::AssetKind;

use crate::admin::mcp::version_list::{is_semver_prerelease, newer_prerelease, newest_releases};
use crate::admin::mcp::views::{TrackedVersion, cached_labels, total_if_truncated};
use crate::admin::mcp::{VeinMcp, json_resource, read_error, unknown_package, uri_param};

#[derive(Serialize)]
struct Crate {
    name: String,
    /// Newest release that is not yanked, per the sparse index.
    #[serde(skip_serializing_if = "Option::is_none")]
    latest: Option<String>,
    /// The newest version not yanked, when it is a prerelease
    /// (`2.0.0-rc.1`).
    #[serde(skip_serializing_if = "Option::is_none")]
    prerelease: Option<String>,
    /// The newest 5 releases, from the sparse index; all of them are at
    /// `.../versions`, each at `.../versions/{v}`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    versions: Vec<String>,
    /// The listed versions that are yanked.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    yanked: Vec<String>,
    /// How many are published, prereleases and yanked ones too, when that
    /// is more than `versions` lists.
    #[serde(skip_serializing_if = "Option::is_none")]
    total_versions: Option<usize>,
    /// Versions whose `.crate` is held here, newest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    cached: Vec<String>,
    /// Versions the release delay policy holds back or decided on, newest
    /// first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    quarantine: Vec<TrackedVersion>,
}

impl VeinMcp {
    /// A crate as seen by this proxy: its newest published versions (from
    /// the sparse index) and the `.crate` files cached here.
    #[mcp_resource_template(
        uri_template = "vein://crates/{name}",
        name = "crate",
        title = "Crate",
        mime_type = "application/json"
    )]
    async fn crate_info(&self, ctx: Ctx<'_>) -> ResourceResult {
        let name = uri_param(&ctx, "name")?;
        let index = self.crate_index(name).await?;
        let cached = self.artifacts(AssetKind::Crate, name).await?;
        let quarantine = self
            .resources
            .tracked_versions(AssetKind::Crate, name)
            .await
            .map_err(read_error)?;
        if index.is_empty() && cached.is_empty() {
            return Err(unknown_package(format_args!("crate {name}")));
        }

        let not_yanked = || {
            index
                .iter()
                .filter(|entry| !entry.yanked)
                .map(|entry| entry.vers.as_str())
        };
        let versions = newest_releases(
            index.iter().map(|entry| entry.vers.as_str()),
            is_semver_prerelease,
        );
        json_resource(
            format!("vein://crates/{name}"),
            &Crate {
                latest: not_yanked()
                    .find(|version| !is_semver_prerelease(version))
                    .map(str::to_owned),
                prerelease: newer_prerelease(not_yanked(), is_semver_prerelease),
                total_versions: total_if_truncated(index.len(), versions.len()),
                yanked: index
                    .iter()
                    .filter(|entry| entry.yanked && versions.contains(&entry.vers))
                    .map(|entry| entry.vers.clone())
                    .collect(),
                versions,
                cached: cached_labels(&cached),
                quarantine: quarantine.into_iter().map(TrackedVersion::from).collect(),
                name: name.to_owned(),
            },
        )
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::crate_info_template_info()?,
        VeinMcp::crate_info_handler,
        None,
    ))
}
