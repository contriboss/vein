use std::collections::BTreeMap;

use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use serde::Serialize;
use vein_adapter::AssetKind;

use crate::admin::mcp::cargo::IndexDependency;
use crate::admin::mcp::{VeinMcp, uri_param};

/// A version's sparse-index entry.
#[derive(Serialize)]
struct IndexDetails {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    yanked: bool,
    /// SHA-256 of the `.crate` file, as published.
    checksum: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    rust_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    links: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    features: BTreeMap<String, Vec<String>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dependencies: Vec<IndexDependency>,
}

impl VeinMcp {
    /// One crate version: dependencies, features, MSRV, checksum, and
    /// whether its `.crate` is cached here.
    #[mcp_resource_template(
        uri_template = "vein://crates/{name}/versions/{version}",
        name = "crate_version",
        title = "Crate version",
        mime_type = "application/json"
    )]
    async fn crate_version(&self, ctx: Ctx<'_>) -> ResourceResult {
        let name = uri_param(&ctx, "name")?;
        let version = uri_param(&ctx, "version")?;
        let published = self
            .crate_index(name)
            .await?
            .into_iter()
            .find(|entry| entry.vers == version)
            .map(|entry| IndexDetails {
                features: entry.all_features(),
                yanked: entry.yanked,
                checksum: entry.cksum,
                rust_version: entry.rust_version,
                links: entry.links,
                dependencies: entry.deps,
            });
        let uri = format!("vein://crates/{name}/versions/{version}");
        self.package_version(AssetKind::Crate, uri, name, version, published)
            .await
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::crate_version_template_info()?,
        VeinMcp::crate_version_handler,
        None,
    ))
}
