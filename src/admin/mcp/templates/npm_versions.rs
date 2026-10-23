use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;

use crate::admin::mcp::version_list::{VersionList, is_semver_prerelease};
use crate::admin::mcp::{VeinMcp, json_resource, unknown_package, uri_param};

impl VeinMcp {
    /// Every version npm publishes of a package: releases and prereleases
    /// apart, newest first, and the deprecated ones.
    #[mcp_resource_template(
        uri_template = "vein://npm/{+name}/versions",
        name = "npm_versions",
        title = "npm package versions",
        mime_type = "application/json"
    )]
    async fn npm_versions(&self, ctx: Ctx<'_>) -> ResourceResult {
        let name = uri_param(&ctx, "name")?;
        let Some(packument) = self.packument(name).await? else {
            return Err(unknown_package(format_args!("npm package {name}")));
        };
        let versions = packument.versions();
        let deprecated: Vec<String> = versions
            .iter()
            .filter(|version| packument.deprecation(version).is_some())
            .map(ToString::to_string)
            .collect();
        let all_deprecated = !versions.is_empty() && deprecated.len() == versions.len();
        json_resource(
            format!("vein://npm/{name}/versions"),
            &VersionList {
                all_deprecated,
                deprecated: if all_deprecated {
                    Vec::new()
                } else {
                    deprecated
                },
                ..VersionList::split(versions.iter().copied(), is_semver_prerelease)
            },
        )
    }
}

pub fn mount(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    Ok(router.with_template(
        VeinMcp::npm_versions_template_info()?,
        VeinMcp::npm_versions_handler,
        None,
    ))
}
