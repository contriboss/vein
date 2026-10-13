use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;
use serde::Serialize;
use vein_adapter::AssetKind;

use crate::admin::mcp::views::Artifact;
use crate::admin::mcp::{VERSION_LIST_LIMIT, VeinMcp, json_resource, not_fetched, uri_param};

#[derive(Serialize)]
struct Crate {
    name: String,
    /// Newest version that is not yanked, per the cached sparse index.
    latest: Option<String>,
    /// Versions in the cached sparse index. Zero when cargo never resolved
    /// this crate through the proxy.
    total_versions: usize,
    /// The newest 50 of them.
    versions: Vec<IndexedVersion>,
    /// `.crate` files held here, newest first.
    cached: Vec<Artifact>,
}

#[derive(Serialize)]
struct IndexedVersion {
    version: String,
    yanked: bool,
    uri: String,
}

impl VeinMcp {
    /// A crate as seen by this proxy: its published versions (from the cached
    /// sparse index) and the `.crate` files cached here.
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
        if index.is_empty() && cached.is_empty() {
            return Err(not_fetched(format_args!("crate {name}")));
        }

        json_resource(
            format!("vein://crates/{name}"),
            &Crate {
                latest: index
                    .iter()
                    .find(|entry| !entry.yanked)
                    .map(|entry| entry.vers.clone()),
                total_versions: index.len(),
                versions: index
                    .into_iter()
                    .take(VERSION_LIST_LIMIT)
                    .map(|entry| IndexedVersion {
                        uri: format!("vein://crates/{name}/versions/{}", entry.vers),
                        version: entry.vers,
                        yanked: entry.yanked,
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
        VeinMcp::crate_info_template_info(),
        VeinMcp::crate_info_handler,
        None,
    )
}
