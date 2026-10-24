//! URI-template resources, one per file.

mod crate_api;
mod crate_api_item;
mod crate_info;
mod crate_readme;
mod crate_search;
mod crate_version;
mod crate_versions;
mod gem;
mod gem_api;
mod gem_api_item;
mod gem_readme;
mod gem_sbom;
mod gem_search;
mod gem_version;
mod gem_versions;
mod npm_api;
mod npm_api_item;
mod npm_package;
mod npm_readme;
mod npm_search;
mod npm_version;
mod npm_versions;

use mcp_host::prelude::AppsError;
use mcp_host::registry::router::McpResourceTemplateRouter;

use super::VeinMcp;

pub const GEM_SEARCH: &str = "vein://search/gems/{query}";
pub const GEM: &str = "vein://gems/{name}";
pub const GEM_VERSIONS: &str = "vein://gems/{name}/versions";
pub const GEM_VERSION: &str = "vein://gems/{name}/versions/{version}";
pub const GEM_README: &str = "vein://gems/{name}/versions/{version}/readme";
pub const GEM_API: &str = "vein://gems/{name}/versions/{version}/api";
pub const GEM_API_ITEM: &str = "vein://gems/{name}/versions/{version}/api/{+path}";
pub const GEM_SBOM: &str = "vein://gems/{name}/versions/{version}/sbom";
pub const CRATE_SEARCH: &str = "vein://search/crates/{query}";
pub const CRATE: &str = "vein://crates/{name}";
pub const CRATE_VERSIONS: &str = "vein://crates/{name}/versions";
pub const CRATE_VERSION: &str = "vein://crates/{name}/versions/{version}";
pub const CRATE_README: &str = "vein://crates/{name}/versions/{version}/readme";
pub const CRATE_API: &str = "vein://crates/{name}/versions/{version}/api";
pub const CRATE_API_ITEM: &str = "vein://crates/{name}/versions/{version}/api/{+path}";
pub const NPM_SEARCH: &str = "vein://search/npm/{query}";
pub const NPM_PACKAGE: &str = "vein://npm/{+name}";
pub const NPM_VERSIONS: &str = "vein://npm/{+name}/versions";
pub const NPM_VERSION: &str = "vein://npm/{+name}/versions/{version}";
pub const NPM_README: &str = "vein://npm/{+name}/versions/{version}/readme";
pub const NPM_API: &str = "vein://npm/{+name}/versions/{version}/api";
pub const NPM_API_ITEM: &str = "vein://npm/{+name}/versions/{version}/api/{+path}";

pub fn router() -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    let router = McpResourceTemplateRouter::new();
    let router = gem_search::mount(router)?;
    let router = gem::mount(router)?;
    let router = gem_versions::mount(router)?;
    let router = gem_version::mount(router)?;
    let router = gem_sbom::mount(router)?;
    let router = gem_readme::mount(router)?;
    let router = gem_api::mount(router)?;
    let router = gem_api_item::mount(router)?;
    let router = crate_search::mount(router)?;
    let router = crate_info::mount(router)?;
    let router = crate_versions::mount(router)?;
    let router = crate_version::mount(router)?;
    let router = crate_readme::mount(router)?;
    let router = crate_api::mount(router)?;
    let router = crate_api_item::mount(router)?;
    let router = npm_search::mount(router)?;
    let router = npm_package::mount(router)?;
    let router = npm_versions::mount(router)?;
    let router = npm_version::mount(router)?;
    let router = npm_readme::mount(router)?;
    let router = npm_api::mount(router)?;
    npm_api_item::mount(router)
}
