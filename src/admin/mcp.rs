//! MCP endpoint (`/mcp`): the proxy's `RubyGems`, crates.io, and npm caches
//! as read-only resources.

mod cargo;
mod completion;
mod npm;
mod resources;
mod templates;
mod views;

use std::{fmt, net::IpAddr, sync::Arc};

use crate::{config::McpSettings, crates::index_storage_path, npm::metadata_storage_path};
use anyhow::{Context, Result};
use mcp_host::{
    macros::{Ctx, ResourceResult, text_resource_with_mime},
    prelude::Server,
    registry::{
        resources::ResourceError,
        router::{McpPromptRouter, McpRouter, McpToolRouter},
    },
    transport::http::{HttpBearerAuthConfig, HttpTransportConfig, McpHttpService},
};
use serde::Serialize;
use vein_adapter::{AssetKind, GemMetadata};

use super::{app::urlencode, state::AdminResources};
use views::{Artifact, PackageVersion, Search};

/// Environment variable holding the bearer token agents must present.
pub const TOKEN_ENV: &str = "VEIN_ADMIN_MCP_TOKEN";

/// Platform assumed when a gem version URI does not name one.
const DEFAULT_PLATFORM: &str = "ruby";

/// Clients that replace a `resources/read` error with their own generic text,
/// so the model never sees why a read failed. For these, mcp-host answers
/// read errors (other than "not found") as a `text/plain` content item.
const ERRORS_AS_CONTENT_CLIENTS: [&str; 1] = ["claude-code < 2.93"];

/// Names returned by a search resource.
const SEARCH_LIMIT: i64 = 50;
/// Published versions listed on a package resource.
const VERSION_LIST_LIMIT: usize = 50;

const INSTRUCTIONS: &str = "Read-only view of this Vein proxy's RubyGems, \
crates.io, and npm caches. Search with vein://gems/search/{query} (every name on \
rubygems.org), vein://crates/search/{query} or vein://npm/search/{query} (packages \
cached here); exact and prefix matches come first. Read a package at \
vein://gems/{name}, vein://crates/{name}, or vein://npm/{name} (scoped names as-is, \
e.g. vein://npm/@types/node), and one version at .../versions/{version}. Gem \
versions take -{platform} for native builds (1.16.0-x86_64-linux) and have \
.../sbom, a CycloneDX SBOM. vein://quarantine lists gem versions held back by the \
release delay policy; vein://stats summarises the whole cache.";

/// Resource handlers get read-only access to the admin's view of the cache.
pub struct VeinMcp {
    resources: AdminResources,
}

/// The `/mcp` service for an admin bound to `host`, or `None` when disabled
/// or when a non-loopback bind has no token. Must run inside a Tokio runtime.
pub fn endpoint(
    settings: &McpSettings,
    host: &str,
    resources: AdminResources,
) -> Result<Option<McpHttpService>> {
    if !settings.enabled {
        return Ok(None);
    }
    let Some(config) = transport_config(host, std::env::var(TOKEN_ENV).ok())? else {
        tracing::warn!(
            host,
            "MCP endpoint off: {host} is not a loopback address and {TOKEN_ENV} is unset"
        );
        return Ok(None);
    };
    service(resources, config).map(Some)
}

/// Transport settings for serving MCP next to an admin bound to `host`, or
/// `None` when it has to stay off.
///
/// Loopback binds are open to local agents. Any other bind needs `token`:
/// embedded, `localhost_only` only filters browser `Origin`s, so a client
/// sending no `Origin` header could otherwise reach it from the network.
pub fn transport_config(host: &str, token: Option<String>) -> Result<Option<HttpTransportConfig>> {
    let loopback = is_loopback(host);
    let bearer_auth = match token.filter(|token| !token.is_empty()) {
        Some(token) => Some(
            HttpBearerAuthConfig::new(token, "agent")
                .context("invalid MCP bearer token")?
                .into(),
        ),
        None if loopback => None,
        None => return Ok(None),
    };
    Ok(Some(HttpTransportConfig {
        localhost_only: loopback,
        bearer_auth,
        ..HttpTransportConfig::default()
    }))
}

/// Build the `/mcp` service. Must run inside a Tokio runtime.
pub fn service(resources: AdminResources, config: HttpTransportConfig) -> Result<McpHttpService> {
    let server = Server::builder("vein", env!("CARGO_PKG_VERSION"))
        .with_title("Vein")
        .with_instructions(INSTRUCTIONS)
        .with_resources(false, false)
        .with_resource_templates()
        .with_completion()
        .with_resource_errors_as_content(ERRORS_AS_CONTENT_CLIENTS)
        .context("invalid MCP client matcher")?
        .build();
    completion::register(&server, &resources);
    let router = McpRouter::new(
        McpToolRouter::new(),
        McpPromptRouter::default(),
        resources::router(),
        templates::router(),
    );
    server.register_router(router, Arc::new(VeinMcp { resources }));
    McpHttpService::new(Arc::new(server), config).context("building MCP endpoint")
}

fn is_loopback(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

impl VeinMcp {
    /// Package files of `name` held here, newest first.
    async fn artifacts(&self, kind: AssetKind, name: &str) -> Result<Vec<Artifact>, ResourceError> {
        let cached = self
            .resources
            .cached_versions(kind, name)
            .await
            .map_err(read_error)?;
        Ok(cached.into_iter().map(Artifact::from).collect())
    }

    /// The `{query}` search over `kind` packages cached here, served at
    /// `vein://{ecosystem}/search/{query}`.
    async fn cached_search(
        &self,
        ctx: &Ctx<'_>,
        kind: AssetKind,
        ecosystem: &str,
    ) -> ResourceResult {
        let query = search_query(ctx)?;
        let names = self
            .resources
            .search_cached(kind, &query, SEARCH_LIMIT)
            .await
            .map_err(read_error)?;
        json_resource(
            search_uri(ecosystem, &query),
            &Search {
                query,
                scope: "cached",
                names,
                catalog_size: None,
            },
        )
    }

    /// Resource for one crate or npm version: what the registry publishes
    /// for it next to the file cached here. Not found when neither exists.
    async fn package_version<D: Serialize>(
        &self,
        kind: AssetKind,
        uri: String,
        name: &str,
        version: &str,
        published: Option<D>,
    ) -> ResourceResult {
        let cached = self
            .artifacts(kind, name)
            .await?
            .into_iter()
            .find(|artifact| artifact.version == version);
        if published.is_none() && cached.is_none() {
            return Err(ResourceError::NotFound(format!(
                "{name} {version} is neither in the cached registry data nor cached"
            )));
        }
        json_resource(
            uri,
            &PackageVersion {
                name: name.to_owned(),
                version: version.to_owned(),
                cached,
                published,
            },
        )
    }

    /// Every version in crate `name`'s cached sparse index, newest first;
    /// empty when cargo never resolved it through the proxy.
    async fn crate_index(&self, name: &str) -> Result<Vec<cargo::IndexEntry>, ResourceError> {
        let path = index_storage_path(name)
            .ok_or_else(|| ResourceError::InvalidUri(format!("`{name}` is not a crate name")))?;
        let bytes = self
            .resources
            .read_stored(&path)
            .await
            .map_err(read_error)?;
        Ok(bytes.as_deref().map(cargo::parse_index).unwrap_or_default())
    }

    /// npm package `name`'s cached registry metadata, if npm ever asked the
    /// proxy for it.
    async fn packument(&self, name: &str) -> Result<Option<npm::Packument>, ResourceError> {
        if !npm::is_valid_name(name) {
            return Err(ResourceError::InvalidUri(format!(
                "`{name}` is not an npm package name"
            )));
        }
        let bytes = self
            .resources
            .read_stored(&metadata_storage_path(name))
            .await
            .map_err(read_error)?;
        Ok(bytes.as_deref().and_then(npm::Packument::parse))
    }

    /// Metadata for a cached version of `name`. `version` may carry a
    /// platform the `RubyGems` way (`1.16.0-x86_64-linux`); otherwise `ruby`.
    async fn cached_metadata(
        &self,
        name: &str,
        version: &str,
    ) -> Result<GemMetadata, ResourceError> {
        let (version, platform) = version
            .split_once('-')
            .unwrap_or((version, DEFAULT_PLATFORM));
        let cached = self
            .resources
            .gem_versions(name)
            .await
            .map_err(read_error)?;
        if !cached.iter().any(|candidate| candidate == version) {
            return Err(ResourceError::NotFound(format!(
                "{name} {version} is not cached; cached versions: [{}]",
                cached.join(", ")
            )));
        }
        self.resources
            .gem_metadata(name, version, Some(platform))
            .await
            .map_err(read_error)?
            .ok_or_else(|| {
                ResourceError::NotFound(format!(
                    "no metadata recorded for {name} {version} ({platform})"
                ))
            })
    }
}

/// A named URI parameter; the router only matches when all are present.
fn uri_param<'c>(ctx: &'c Ctx<'_>, name: &str) -> Result<&'c str, ResourceError> {
    ctx.get_uri_param(name)
        .ok_or_else(|| ResourceError::InvalidUri(format!("missing `{name}`")))
}

/// The trimmed, non-empty `{query}` of a search resource.
fn search_query(ctx: &Ctx<'_>) -> Result<String, ResourceError> {
    let query = uri_param(ctx, "query")?.trim();
    if query.is_empty() {
        return Err(ResourceError::InvalidUri("empty search query".into()));
    }
    Ok(query.to_string())
}

/// Canonical URI of the `ecosystem` search for `query`.
fn search_uri(ecosystem: &str, query: &str) -> String {
    format!("vein://{ecosystem}/search/{}", urlencode(query))
}

/// `value` as the pretty-printed JSON body of resource `uri`.
fn json_resource<T: Serialize>(uri: impl Into<String>, value: &T) -> ResourceResult {
    json_resource_as(uri, value, "application/json")
}

/// `value` as pretty-printed JSON served with a more specific `mime_type`.
fn json_resource_as<T: Serialize>(
    uri: impl Into<String>,
    value: &T,
    mime_type: &'static str,
) -> ResourceResult {
    let body = serde_json::to_string_pretty(value)
        .map_err(|err| ResourceError::Internal(err.to_string()))?;
    Ok(vec![text_resource_with_mime(uri, body, mime_type)])
}

/// A package neither cached nor ever resolved through the proxy.
fn not_fetched(package: impl fmt::Display) -> ResourceError {
    ResourceError::NotFound(format!("{package} has not been fetched through this proxy"))
}

#[expect(clippy::needless_pass_by_value, reason = "used as a `map_err` adapter")]
fn read_error(err: anyhow::Error) -> ResourceError {
    ResourceError::Read(format!("{err:#}"))
}

#[cfg(test)]
mod tests;
