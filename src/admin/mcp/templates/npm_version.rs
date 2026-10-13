use std::collections::BTreeMap;

use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use serde::Serialize;
use vein_adapter::AssetKind;

use crate::admin::mcp::npm::{license, string_map, text};
use crate::admin::mcp::{VeinMcp, uri_param};

/// A version's manifest from the cached registry metadata.
#[derive(Serialize)]
struct Manifest {
    description: Option<String>,
    license: Option<String>,
    /// Deprecation message, if the author deprecated this version.
    deprecated: Option<String>,
    dependencies: BTreeMap<String, String>,
    peer_dependencies: BTreeMap<String, String>,
    optional_dependencies: BTreeMap<String, String>,
    engines: BTreeMap<String, String>,
    /// Subresource-integrity hash of the published tarball.
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
            Some(Manifest {
                description: field("description").and_then(text),
                license: field("license").and_then(license),
                deprecated: field("deprecated").and_then(text),
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

pub fn mount(router: McpResourceTemplateRouter<VeinMcp>) -> McpResourceTemplateRouter<VeinMcp> {
    router.with_template(
        VeinMcp::npm_version_template_info(),
        VeinMcp::npm_version_handler,
        None,
    )
}
