//! Package search through vein, in each registry's own wire format, for
//! clients (`cargo search --registry vein`, `npm search`, `RubyGems`' search
//! API) and for vein's MCP, which reads it over loopback.
//!
//! The registry's own search answers when it can: ranked by relevance and
//! downloads, passed through untouched. Out of reach, the answer comes from
//! vein's name catalog (`[catalog] registries`), else from the names cached
//! here. [`SOURCE_HEADER`] says which.

use std::time::Duration;

use anyhow::{Context, Result};
use rama::http::{Body, HeaderValue, Response, StatusCode, client::DefaultHttpWebClient, header};
use rama::net::uri::Uri;
use rama::telemetry::tracing::warn;
use serde_json::{Value, json};
use vein_adapter::{AssetKind, CacheBackend, CacheBackendTrait, CatalogName};

use crate::upstream::{buffer_response, simple_get};

/// `registry`, `catalog` or `cached`: where the results came from.
pub const SOURCE_HEADER: &str = "x-vein-search";
/// How long the registry gets before vein answers from its catalog.
const REGISTRY_TIMEOUT: Duration = Duration::from_secs(10);

/// Where each registry's search lives.
#[derive(Debug, Clone)]
pub struct Registries {
    /// rubygems.org (vein's configured upstream), without a trailing `/`.
    pub gems: String,
    pub crates: String,
    pub npm: String,
}

impl Default for Registries {
    fn default() -> Self {
        Self {
            gems: "https://rubygems.org".into(),
            crates: "https://crates.io".into(),
            npm: "https://registry.npmjs.org".into(),
        }
    }
}

/// A search request in one registry's dialect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub kind: AssetKind,
    pub text: String,
    pub limit: usize,
}

impl Query {
    /// The search request `uri` is, if it is one: `/api/v1/search.json?query=`
    /// (`RubyGems`), `/api/v1/crates?q=` (cargo), `/-/v1/search?text=` (npm).
    #[must_use]
    pub fn parse(uri: &Uri) -> Option<Self> {
        let (kind, text_key, limit_key, default_limit, max_limit) = match &*uri.path_or_root() {
            "/api/v1/search.json" => (AssetKind::Gem, "query", None, 30, 30),
            "/api/v1/crates" => (AssetKind::Crate, "q", Some("per_page"), 10, 100),
            "/-/v1/search" => (AssetKind::NpmPackage, "text", Some("size"), 20, 250),
            _ => return None,
        };
        let text = uri.first_query_value(text_key)?.trim().to_owned();
        let limit = limit_key
            .and_then(|key| uri.first_query_value(key))
            .and_then(|limit| limit.parse::<usize>().ok())
            .unwrap_or(default_limit)
            .clamp(1, max_limit);
        Some(Self { kind, text, limit })
    }
}

/// The answer to `query`, whose request line was `path_and_query`.
pub async fn respond(
    client: &DefaultHttpWebClient,
    index: &CacheBackend,
    registries: &Registries,
    query: &Query,
    path_and_query: &str,
) -> Result<Response<Body>> {
    let base = match query.kind {
        AssetKind::Gem | AssetKind::Spec => &registries.gems,
        AssetKind::Crate => &registries.crates,
        AssetKind::NpmPackage => &registries.npm,
    };
    let url = format!("{base}{path_and_query}");
    let asked = tokio::time::timeout(
        REGISTRY_TIMEOUT,
        simple_get(
            client,
            &url,
            &header::HeaderMap::new(),
            Some("application/json"),
        ),
    )
    .await;
    match asked {
        Ok(Ok(response)) if response.status().is_success() => {
            let response = buffer_response(response, true).await?;
            return with_exact_match(index, query, response).await;
        }
        Ok(Ok(response)) => {
            warn!(status = %response.status(), %url, "search: registry refused, answering from the catalog");
        }
        Ok(Err(err)) => {
            warn!(error = %err, %url, "search: registry unreachable, answering from the catalog");
        }
        Err(_) => warn!(%url, "search: registry too slow, answering from the catalog"),
    }

    let limit = i64::try_from(query.limit).unwrap_or(i64::MAX);
    let mut names = index.names_search(query.kind, &query.text, limit).await?;
    let mut source = "catalog";
    if names.is_empty() && index.names_count(query.kind).await? == 0 {
        source = "cached";
        names = index
            .search_cached(query.kind, &query.text, limit)
            .await?
            .into_iter()
            .map(|name| CatalogName {
                name,
                latest_version: None,
            })
            .collect();
    }
    let body = in_registry_format(query.kind, &names);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(SOURCE_HEADER, source)
        .body(Body::from(body.to_string()))
        .context("building search response")
}

/// The registry's answer, with the package named exactly as the query first
/// when the answer lacks it and the catalog knows it (npm's search leaves
/// out deprecated packages: `left-pad` was not among its own results).
/// Otherwise the registry's bytes, untouched.
async fn with_exact_match(
    index: &CacheBackend,
    query: &Query,
    response: Response<Body>,
) -> Result<Response<Body>> {
    use rama::http::body::util::BodyExt;

    let (mut parts, body) = response.into_parts();
    parts
        .headers
        .insert(SOURCE_HEADER, HeaderValue::from_static("registry"));
    let bytes = body
        .collect()
        .await
        .context("reading the registry's search answer")?
        .to_bytes();
    let exact = |name: &str| match query.kind {
        // rubygems.org publishes both `Rack` and `rack`.
        AssetKind::Gem | AssetKind::Spec => name == query.text,
        AssetKind::Crate | AssetKind::NpmPackage => name.eq_ignore_ascii_case(&query.text),
    };
    let Ok(mut answer) = serde_json::from_slice::<Value>(&bytes) else {
        return Ok(Response::from_parts(parts, Body::from(bytes)));
    };
    if names(query.kind, &answer).iter().any(|name| exact(name)) {
        return Ok(Response::from_parts(parts, Body::from(bytes)));
    }
    let found = match index.names_search(query.kind, &query.text, 1).await {
        Ok(found) => found.into_iter().find(|entry| exact(&entry.name)),
        Err(err) => {
            warn!(error = %err, "search: catalog unavailable, the registry's answer as is");
            None
        }
    };
    let Some(entry) = found else {
        return Ok(Response::from_parts(parts, Body::from(bytes)));
    };
    prepend(query.kind, &mut answer, &entry);
    parts.headers.remove(header::CONTENT_LENGTH);
    Ok(Response::from_parts(parts, Body::from(answer.to_string())))
}

/// `entry` as the first hit of a registry search `answer`.
fn prepend(kind: AssetKind, answer: &mut Value, entry: &CatalogName) {
    let mut hit = in_registry_format(kind, std::slice::from_ref(entry));
    let (list, hit) = match kind {
        AssetKind::Gem | AssetKind::Spec => {
            (answer.as_array_mut(), hit.get_mut(0).map(Value::take))
        }
        AssetKind::Crate => (
            answer.get_mut("crates").and_then(Value::as_array_mut),
            hit.pointer_mut("/crates/0").map(Value::take),
        ),
        AssetKind::NpmPackage => (
            answer.get_mut("objects").and_then(Value::as_array_mut),
            hit.pointer_mut("/objects/0").map(Value::take),
        ),
    };
    if let (Some(list), Some(hit)) = (list, hit) {
        list.insert(0, hit);
    }
}

/// `names` the way the registry's own search would list them, as far as
/// a name and a version go.
#[must_use]
pub fn in_registry_format(kind: AssetKind, names: &[CatalogName]) -> Value {
    let version = |entry: &CatalogName| entry.latest_version.clone().unwrap_or_default();
    match kind {
        AssetKind::Gem | AssetKind::Spec => Value::Array(
            names
                .iter()
                .map(|entry| json!({"name": entry.name, "version": version(entry)}))
                .collect(),
        ),
        // cargo reads `name`, `max_version` and `description`.
        AssetKind::Crate => json!({
            "crates": names.iter().map(|entry| json!({
                "name": entry.name, "max_version": version(entry), "description": null,
            })).collect::<Vec<_>>(),
            "meta": {"total": names.len()},
        }),
        AssetKind::NpmPackage => json!({
            "objects": names.iter().map(|entry| json!({
                "package": {"name": entry.name, "version": version(entry)},
            })).collect::<Vec<_>>(),
            "total": names.len(),
        }),
    }
}

/// The names in a registry search answer, in its order.
#[must_use]
pub fn names(kind: AssetKind, body: &Value) -> Vec<String> {
    let list = match kind {
        AssetKind::Gem | AssetKind::Spec => body.as_array(),
        AssetKind::Crate => body.get("crates").and_then(Value::as_array),
        AssetKind::NpmPackage => body.get("objects").and_then(Value::as_array),
    };
    list.into_iter()
        .flatten()
        .filter_map(|hit| match kind {
            AssetKind::NpmPackage => hit.get("package")?.get("name")?.as_str(),
            _ => hit.get("name")?.as_str(),
        })
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(test)]
mod tests;
