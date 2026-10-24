use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use serde::Serialize;
use vein_adapter::AssetKind;

use crate::admin::mcp::rubygems::is_prerelease;
use crate::admin::mcp::version_list::{newer_prerelease, newest_releases};
use crate::admin::mcp::views::{TrackedVersion, total_if_truncated};
use crate::admin::mcp::{
    DEFAULT_PLATFORM, VeinMcp, json_resource, read_error, unknown_package, uri_param,
};
use crate::untrusted::{GemText, SUMMARY_LIMIT, clean_list, clean_opt};

#[derive(Serialize)]
struct Gem {
    name: String,
    /// Only when rubygems.org does not publish this name (per the synced
    /// catalog): it was served from somewhere else.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    not_in_catalog: bool,
    /// Newest published release (prereleases skipped), per rubygems.org.
    #[serde(skip_serializing_if = "Option::is_none")]
    latest: Option<String>,
    /// The newest version, when it is a prerelease (`8.1.0.rc1`).
    #[serde(skip_serializing_if = "Option::is_none")]
    prerelease: Option<String>,
    /// The newest 5 releases, from the compact index; all of them are at
    /// `.../versions`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    versions: Vec<String>,
    /// How many are published, prereleases too, when that is more than
    /// `versions` lists.
    #[serde(skip_serializing_if = "Option::is_none")]
    total_versions: Option<usize>,
    /// Versions cached here, newest first; each is at `.../versions/{v}`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    cached: Vec<String>,
    /// Versions the release delay policy tracks, newest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    quarantine: Vec<TrackedVersion>,
    /// The newest cached version's summary and licenses, as its authors
    /// wrote them.
    #[serde(skip_serializing_if = "GemText::is_empty")]
    author_supplied: GemText,
}

impl VeinMcp {
    /// A gem: its published versions (rubygems.org's compact index, looked
    /// up through the proxy when nobody fetched it yet), the versions cached
    /// here and their quarantine status.
    #[mcp_resource_template(
        uri_template = "vein://gems/{name}",
        name = "gem",
        title = "Gem",
        mime_type = "application/json"
    )]
    async fn gem(&self, ctx: Ctx<'_>) -> ResourceResult {
        let name = uri_param(&ctx, "name")?;
        let published = self.gem_info(name).await?;
        let cached = self
            .resources
            .gem_versions(name)
            .await
            .map_err(read_error)?;
        if published.is_empty() && cached.is_empty() {
            return Err(unknown_package(format_args!("gem {name}")));
        }
        let quarantine = self
            .resources
            .tracked_versions(AssetKind::Gem, name)
            .await
            .map_err(read_error)?;
        let in_catalog = self
            .resources
            .catalog_search(name, 1)
            .await
            .map_err(read_error)?
            .first()
            .is_some_and(|found| found == name);
        let newest = match cached.first() {
            Some(version) => self
                .resources
                .gem_metadata(name, version, Some(DEFAULT_PLATFORM))
                .await
                .map_err(read_error)?,
            None => None,
        };
        let author_supplied = newest.map_or_else(GemText::default, |meta| GemText {
            summary: clean_opt(meta.summary, SUMMARY_LIMIT),
            licenses: clean_list(meta.licenses),
            ..GemText::default()
        });

        let newest_first = || published.iter().map(String::as_str);
        let versions = newest_releases(newest_first(), is_prerelease);
        json_resource(
            format!("vein://gems/{name}"),
            &Gem {
                latest: versions.first().cloned(),
                prerelease: newer_prerelease(newest_first(), is_prerelease),
                total_versions: total_if_truncated(published.len(), versions.len()),
                versions,
                cached,
                quarantine: quarantine.into_iter().map(TrackedVersion::from).collect(),
                not_in_catalog: !in_catalog,
                author_supplied,
                name: name.to_owned(),
            },
        )
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(VeinMcp::gem_template_info()?, VeinMcp::gem_handler, None))
}
