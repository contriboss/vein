use std::collections::BTreeMap;

use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use serde::Serialize;
use vein_adapter::AssetKind;

use crate::admin::mcp::views::Artifact;
use crate::admin::mcp::{VERSION_LIST_LIMIT, VeinMcp, json_resource, not_fetched, uri_param};

#[derive(Serialize)]
struct Package {
    name: String,
    description: Option<String>,
    license: Option<String>,
    homepage: Option<String>,
    repository: Option<String>,
    /// e.g. `{"latest": "4.17.21", "next": "5.0.0-rc.1"}`.
    dist_tags: BTreeMap<String, String>,
    /// Versions in the cached registry metadata. Zero when npm never asked
    /// the proxy for this package's metadata.
    total_versions: usize,
    /// The newest 50 of them.
    versions: Vec<PublishedVersion>,
    /// Tarballs held here, newest first.
    cached: Vec<Artifact>,
}

#[derive(Serialize)]
struct PublishedVersion {
    version: String,
    deprecated: bool,
    uri: String,
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
        if packument.is_none() && cached.is_empty() {
            return Err(not_fetched(format_args!("npm package {name}")));
        }

        let packument = packument.as_ref();
        let versions = packument.map(|p| p.versions()).unwrap_or_default();
        json_resource(
            format!("vein://npm/{name}"),
            &Package {
                description: packument.and_then(|p| p.text("description")),
                license: packument.and_then(super::super::npm::Packument::license),
                homepage: packument.and_then(|p| p.text("homepage")),
                repository: packument.and_then(super::super::npm::Packument::repository),
                dist_tags: packument
                    .map(super::super::npm::Packument::dist_tags)
                    .unwrap_or_default(),
                total_versions: versions.len(),
                versions: versions
                    .into_iter()
                    .take(VERSION_LIST_LIMIT)
                    .map(|version| PublishedVersion {
                        deprecated: packument
                            .and_then(|p| p.manifest(version))
                            .is_some_and(|manifest| manifest.contains_key("deprecated")),
                        uri: format!("vein://npm/{name}/versions/{version}"),
                        version: version.to_string(),
                    })
                    .collect(),
                cached,
                name: name.to_owned(),
            },
        )
    }
}

pub fn mount(router: McpResourceTemplateRouter<VeinMcp>) -> McpResourceTemplateRouter<VeinMcp> {
    router.with_template(
        VeinMcp::npm_package_template_info(),
        VeinMcp::npm_package_handler,
        None,
    )
}
