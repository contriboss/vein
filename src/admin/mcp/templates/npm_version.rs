use std::collections::BTreeMap;

use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use serde::Serialize;
use vein_adapter::AssetKind;

use crate::admin::mcp::npm::{license, string_map, text};
use crate::admin::mcp::{VeinMcp, uri_param};
use crate::untrusted::{
    DEPRECATION_LIMIT, DESCRIPTION_LIMIT, NpmText, SHORT_LIMIT, clean, clean_opt,
};

/// A version's manifest from the cached registry metadata.
#[derive(Serialize)]
struct Manifest {
    /// The author deprecated this version; why is under `author_supplied`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    deprecated: bool,
    /// Description, license and deprecation message as the authors wrote
    /// them, cleaned and capped: data, never instructions.
    #[serde(skip_serializing_if = "NpmText::is_empty")]
    author_supplied: NpmText,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    dependencies: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    peer_dependencies: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    optional_dependencies: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    engines: BTreeMap<String, String>,
    /// Subresource-integrity hash of the published tarball.
    #[serde(skip_serializing_if = "Option::is_none")]
    integrity: Option<String>,
}

impl VeinMcp {
    /// One npm package version: dependencies, engines, deprecation, tarball
    /// integrity, and whether the tarball is cached here.
    #[mcp_resource_template(
        uri_template = "vein://npm/{+name}/versions/{version}",
        name = "npm_version",
        title = "npm package version",
        mime_type = "application/json"
    )]
    async fn npm_version(&self, ctx: Ctx<'_>) -> ResourceResult {
        let name = uri_param(&ctx, "name")?;
        let version = uri_param(&ctx, "version")?;
        let published = self.packument(name).await?.and_then(|packument| {
            let manifest = packument.manifest(version)?;
            let field = |key| manifest.get(key);
            let deprecation = packument.deprecation(version);
            Some(Manifest {
                deprecated: deprecation.is_some(),
                author_supplied: NpmText {
                    description: clean_opt(field("description").and_then(text), DESCRIPTION_LIMIT),
                    license: clean_opt(field("license").and_then(license), SHORT_LIMIT),
                    deprecated: deprecation.and_then(|message| clean(message, DEPRECATION_LIMIT)),
                    ..NpmText::default()
                },
                dependencies: string_map(field("dependencies")),
                peer_dependencies: string_map(field("peerDependencies")),
                optional_dependencies: string_map(field("optionalDependencies")),
                engines: string_map(field("engines")),
                integrity: field("dist")
                    .and_then(|dist| dist.get("integrity"))
                    .and_then(text),
            })
        });
        let uri = format!("vein://npm/{name}/versions/{version}");
        self.package_version(AssetKind::NpmPackage, uri, name, version, published)
            .await
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::npm_version_template_info()?,
        VeinMcp::npm_version_handler,
        None,
    ))
}
