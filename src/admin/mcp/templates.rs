//! URI-template resources, one per file.

#[cfg(feature = "crates")]
mod crate_api;
#[cfg(feature = "crates")]
mod crate_api_item;
#[cfg(feature = "crates")]
mod crate_info;
#[cfg(feature = "crates")]
mod crate_readme;
#[cfg(feature = "crates")]
mod crate_search;
#[cfg(feature = "crates")]
mod crate_version;
#[cfg(feature = "crates")]
mod crate_versions;
#[cfg(feature = "gems")]
mod gem;
#[cfg(feature = "gems")]
mod gem_api;
#[cfg(feature = "gems")]
mod gem_api_item;
#[cfg(feature = "gems")]
mod gem_readme;
#[cfg(feature = "gems")]
mod gem_sbom;
#[cfg(feature = "gems")]
mod gem_search;
#[cfg(feature = "gems")]
mod gem_version;
#[cfg(feature = "gems")]
mod gem_versions;
#[cfg(feature = "npm")]
mod npm_api;
#[cfg(feature = "npm")]
mod npm_api_item;
#[cfg(feature = "npm")]
mod npm_package;
#[cfg(feature = "npm")]
mod npm_readme;
#[cfg(feature = "npm")]
mod npm_search;
#[cfg(feature = "npm")]
mod npm_version;
#[cfg(feature = "npm")]
mod npm_versions;

use mcp_host::prelude::AppsError;
use mcp_host::registry::router::McpResourceTemplateRouter;
use vein_adapter::AssetKind;

use super::VeinMcp;

#[cfg(feature = "gems")]
pub const GEM_SEARCH: &str = "vein://search/gems/{query}";
#[cfg(feature = "gems")]
pub const GEM: &str = "vein://gems/{name}";
#[cfg(feature = "gems")]
pub const GEM_VERSIONS: &str = "vein://gems/{name}/versions";
#[cfg(feature = "gems")]
pub const GEM_VERSION: &str = "vein://gems/{name}/versions/{version}";
#[cfg(feature = "gems")]
pub const GEM_README: &str = "vein://gems/{name}/versions/{version}/readme";
#[cfg(feature = "gems")]
pub const GEM_API: &str = "vein://gems/{name}/versions/{version}/api";
#[cfg(feature = "gems")]
pub const GEM_API_ITEM: &str = "vein://gems/{name}/versions/{version}/api/{+path}";
#[cfg(feature = "gems")]
pub const GEM_SBOM: &str = "vein://gems/{name}/versions/{version}/sbom";
#[cfg(feature = "crates")]
pub const CRATE_SEARCH: &str = "vein://search/crates/{query}";
#[cfg(feature = "crates")]
pub const CRATE: &str = "vein://crates/{name}";
#[cfg(feature = "crates")]
pub const CRATE_VERSIONS: &str = "vein://crates/{name}/versions";
#[cfg(feature = "crates")]
pub const CRATE_VERSION: &str = "vein://crates/{name}/versions/{version}";
#[cfg(feature = "crates")]
pub const CRATE_README: &str = "vein://crates/{name}/versions/{version}/readme";
#[cfg(feature = "crates")]
pub const CRATE_API: &str = "vein://crates/{name}/versions/{version}/api";
#[cfg(feature = "crates")]
pub const CRATE_API_ITEM: &str = "vein://crates/{name}/versions/{version}/api/{+path}";
#[cfg(feature = "npm")]
pub const NPM_SEARCH: &str = "vein://search/npm/{query}";
#[cfg(feature = "npm")]
pub const NPM_PACKAGE: &str = "vein://npm/{+name}";
#[cfg(feature = "npm")]
pub const NPM_VERSIONS: &str = "vein://npm/{+name}/versions";
#[cfg(feature = "npm")]
pub const NPM_VERSION: &str = "vein://npm/{+name}/versions/{version}";
#[cfg(feature = "npm")]
pub const NPM_README: &str = "vein://npm/{+name}/versions/{version}/readme";
#[cfg(feature = "npm")]
pub const NPM_API: &str = "vein://npm/{+name}/versions/{version}/api";
#[cfg(feature = "npm")]
pub const NPM_API_ITEM: &str = "vein://npm/{+name}/versions/{version}/api/{+path}";

/// Registry `kind`'s templates: none for one this build left out.
#[must_use]
pub const fn of(kind: AssetKind) -> &'static [&'static str] {
    match kind {
        #[cfg(feature = "gems")]
        AssetKind::Gem | AssetKind::Spec => &[
            GEM_SEARCH,
            GEM,
            GEM_VERSIONS,
            GEM_VERSION,
            GEM_SBOM,
            GEM_README,
            GEM_API,
            GEM_API_ITEM,
        ],
        #[cfg(feature = "crates")]
        AssetKind::Crate => &[
            CRATE_SEARCH,
            CRATE,
            CRATE_VERSIONS,
            CRATE_VERSION,
            CRATE_README,
            CRATE_API,
            CRATE_API_ITEM,
        ],
        #[cfg(feature = "npm")]
        AssetKind::NpmPackage => &[
            NPM_SEARCH,
            NPM_PACKAGE,
            NPM_VERSIONS,
            NPM_VERSION,
            NPM_README,
            NPM_API,
            NPM_API_ITEM,
        ],
        #[allow(unreachable_patterns)]
        _ => &[],
    }
}

pub fn router() -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    let router = McpResourceTemplateRouter::new();
    #[cfg(feature = "gems")]
    let router = gems(router)?;
    #[cfg(feature = "crates")]
    let router = crates(router)?;
    #[cfg(feature = "npm")]
    let router = npm(router)?;
    Ok(router)
}

#[cfg(feature = "gems")]
fn gems(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    let router = gem_search::mount(router)?;
    let router = gem::mount(router)?;
    let router = gem_versions::mount(router)?;
    let router = gem_version::mount(router)?;
    let router = gem_sbom::mount(router)?;
    let router = gem_readme::mount(router)?;
    let router = gem_api::mount(router)?;
    gem_api_item::mount(router)
}

#[cfg(feature = "crates")]
fn crates(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    let router = crate_search::mount(router)?;
    let router = crate_info::mount(router)?;
    let router = crate_versions::mount(router)?;
    let router = crate_version::mount(router)?;
    let router = crate_readme::mount(router)?;
    let router = crate_api::mount(router)?;
    crate_api_item::mount(router)
}

#[cfg(feature = "npm")]
fn npm(
    router: McpResourceTemplateRouter<VeinMcp>,
) -> Result<McpResourceTemplateRouter<VeinMcp>, AppsError> {
    let router = npm_search::mount(router)?;
    let router = npm_package::mount(router)?;
    let router = npm_versions::mount(router)?;
    let router = npm_version::mount(router)?;
    let router = npm_readme::mount(router)?;
    let router = npm_api::mount(router)?;
    npm_api_item::mount(router)
}
