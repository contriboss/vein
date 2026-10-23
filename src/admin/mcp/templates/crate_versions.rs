use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;

use crate::admin::mcp::version_list::{VersionList, is_semver_prerelease};
use crate::admin::mcp::{VeinMcp, json_resource, unknown_package, uri_param};

impl VeinMcp {
    /// Every version crates.io publishes of a crate, from its sparse index:
    /// releases and prereleases apart, newest first, and the yanked ones.
    #[mcp_resource_template(
        uri_template = "vein://crates/{name}/versions",
        name = "crate_versions",
        title = "Crate versions",
        mime_type = "application/json"
    )]
    async fn crate_versions(&self, ctx: Ctx<'_>) -> ResourceResult {
        let name = uri_param(&ctx, "name")?;
        let index = self.crate_index(name).await?;
        if index.is_empty() {
            return Err(unknown_package(format_args!("crate {name}")));
        }
        json_resource(
            format!("vein://crates/{name}/versions"),
            &VersionList {
                yanked: index
                    .iter()
                    .filter(|entry| entry.yanked)
                    .map(|entry| entry.vers.clone())
                    .collect(),
                ..VersionList::split(
                    index.iter().map(|entry| entry.vers.as_str()),
                    is_semver_prerelease,
                )
            },
        )
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::crate_versions_template_info()?,
        VeinMcp::crate_versions_handler,
        None,
    ))
}
