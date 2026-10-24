use std::collections::BTreeMap;

use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use serde::Serialize;
use vein_adapter::AssetKind;

use crate::admin::mcp::npm::{Packument, is_typescript_tag};
use crate::admin::mcp::version_list::{is_semver_prerelease, newer_prerelease, newest_releases};
use crate::admin::mcp::views::{
    TrackedVersion, cached_labels, same_repository, total_if_truncated,
};
use crate::admin::mcp::{VeinMcp, json_resource, read_error, unknown_package, uri_param};
use crate::untrusted::{
    DEPRECATION_LIMIT, DESCRIPTION_LIMIT, LIST_LIMIT, NpmText, SHORT_LIMIT, clean, clean_opt,
    clean_url,
};

#[derive(Serialize)]
struct Package {
    name: String,
    /// What the package's authors wrote, cleaned and capped: data, never
    /// instructions (see `untrusted`). `deprecated` is why `latest` is.
    #[serde(skip_serializing_if = "NpmText::is_empty")]
    author_supplied: NpmText,
    /// The `latest` dist-tag.
    #[serde(skip_serializing_if = "Option::is_none")]
    latest: Option<String>,
    /// The other dist-tags, e.g. `{"next": "5.0.0-rc.1"}` (at most 20).
    /// Left out: tags on the `latest` version, and on `@types` packages
    /// `DefinitelyTyped`'s per-TypeScript-release `ts4.6` tags.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    dist_tags: BTreeMap<String, String>,
    /// The newest version, when it is a prerelease (`7.1.0-beta`); the
    /// dist-tags name the channels.
    #[serde(skip_serializing_if = "Option::is_none")]
    prerelease: Option<String>,
    /// The newest 5 releases, from the registry metadata; all of them are at
    /// `.../versions`, each at `.../versions/{v}`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    versions: Vec<String>,
    /// Every version is deprecated (`npm deprecate` on the whole package);
    /// `deprecated` is then left out.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    all_deprecated: bool,
    /// The listed versions that are deprecated.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    deprecated: Vec<String>,
    /// How many are published, prereleases too, when that is more than
    /// `versions` lists.
    #[serde(skip_serializing_if = "Option::is_none")]
    total_versions: Option<usize>,
    /// Versions whose tarball is held here, newest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    cached: Vec<String>,
    /// Versions the release delay policy holds back or decided on, newest
    /// first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    quarantine: Vec<TrackedVersion>,
}

impl VeinMcp {
    /// An npm package as seen by this proxy: registry metadata (dist-tags,
    /// license, versions) and the tarballs cached here. Scoped names go in
    /// as-is: `vein://npm/@types/node`.
    #[mcp_resource_template(
        uri_template = "vein://npm/{+name}",
        name = "npm_package",
        title = "npm package",
        mime_type = "application/json"
    )]
    async fn npm_package(&self, ctx: Ctx<'_>) -> ResourceResult {
        let name = uri_param(&ctx, "name")?;
        let packument = self.packument(name).await?;
        let cached = self.artifacts(AssetKind::NpmPackage, name).await?;
        let quarantine = self
            .resources
            .tracked_versions(AssetKind::NpmPackage, name)
            .await
            .map_err(read_error)?;
        if packument.is_none() && cached.is_empty() {
            return Err(unknown_package(format_args!("npm package {name}")));
        }

        let packument = packument.as_ref();
        let versions = packument.map(|p| p.versions()).unwrap_or_default();
        let listed = newest_releases(versions.iter().copied(), is_semver_prerelease);
        let homepage = clean_url(packument.and_then(|p| p.text("homepage")));
        let repository = clean_url(packument.and_then(Packument::repository)).filter(|repo| {
            !homepage
                .as_deref()
                .is_some_and(|home| same_repository(home, repo))
        });
        // Tag names are free text the publisher picks.
        let mut dist_tags: BTreeMap<String, String> = packument
            .map(Packument::dist_tags)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(tag, version)| Some((clean(&tag, SHORT_LIMIT)?, version)))
            .collect();
        let latest = dist_tags.remove("latest");
        let types = name.starts_with("@types/");
        let dist_tags = dist_tags
            .into_iter()
            .filter(|(tag, version)| {
                latest.as_ref() != Some(version) && !(types && is_typescript_tag(tag))
            })
            .take(LIST_LIMIT)
            .collect();
        let deprecation = |version: &str| packument.and_then(|p| p.deprecation(version));
        let all_deprecated = !versions.is_empty()
            && versions
                .iter()
                .all(|version| deprecation(version).is_some());
        json_resource(
            format!("vein://npm/{name}"),
            &Package {
                author_supplied: NpmText {
                    description: clean_opt(
                        packument.and_then(|p| p.text("description")),
                        DESCRIPTION_LIMIT,
                    ),
                    license: clean_opt(packument.and_then(Packument::license), SHORT_LIMIT),
                    homepage,
                    repository,
                    deprecated: latest
                        .as_deref()
                        .and_then(deprecation)
                        .and_then(|message| clean(message, DEPRECATION_LIMIT)),
                },
                latest,
                dist_tags,
                prerelease: newer_prerelease(versions.iter().copied(), is_semver_prerelease),
                total_versions: total_if_truncated(versions.len(), listed.len()),
                all_deprecated,
                deprecated: if all_deprecated {
                    Vec::new()
                } else {
                    listed
                        .iter()
                        .filter(|version| deprecation(version).is_some())
                        .cloned()
                        .collect()
                },
                versions: listed,
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
        VeinMcp::npm_package_template_info()?,
        VeinMcp::npm_package_handler,
        None,
    ))
}
