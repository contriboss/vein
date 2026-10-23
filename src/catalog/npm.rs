//! npm's catalog, from its replication endpoint: one walk of `_all_docs`
//! (resumed where it stopped if vein restarts), then the `_changes` feed
//! from the sequence the walk started at, so nothing published during it
//! is missed and unpublished packages leave.

use std::time::Duration;

use anyhow::{Result, anyhow};
use rama::{
    Service,
    error::extra::OpaqueError,
    http::{BodyExtractExt, Request, Response, service::client::HttpClientExt as _},
};
use serde::Deserialize;
use serde_json::Value;
use vein_adapter::{AssetKind, CacheBackend, CacheBackendTrait, CatalogName};

pub const REPLICATE: &str = "https://replicate.npmjs.com";
/// Last document id the walk read; the next page starts after it.
const META_WALK: &str = "catalog_npm_walk";
/// The sequence the walk started at; `_changes` continues from it.
const META_WALK_SEQ: &str = "catalog_npm_walk_seq";
/// The sequence `_changes` reached; set once the walk finished.
const META_SEQ: &str = "catalog_npm_seq";
const PAGE: usize = 10_000;

#[derive(Deserialize)]
struct Root {
    update_seq: Value,
}

#[derive(Deserialize)]
struct AllDocs {
    rows: Vec<Row>,
}

#[derive(Deserialize)]
struct Row {
    id: String,
}

#[derive(Deserialize)]
struct Changes {
    results: Vec<Change>,
    last_seq: Value,
}

#[derive(Deserialize)]
struct Change {
    id: String,
    #[serde(default)]
    deleted: bool,
}

/// Names synced (added or removed).
pub async fn sync(
    index: &CacheBackend,
    client: &impl Service<Request, Output = Response, Error = OpaqueError>,
    base: &str,
    pace: Duration,
) -> Result<usize> {
    if let Some(seq) = index.catalog_meta_get(META_SEQ).await? {
        return changes(index, client, base, pace, seq).await;
    }
    let seq = if let Some(seq) = index.catalog_meta_get(META_WALK_SEQ).await? {
        seq
    } else {
        let root: Root = get(client, &format!("{base}/")).await?;
        let seq = sequence(&root.update_seq);
        index.catalog_meta_set(META_WALK_SEQ, &seq).await?;
        seq
    };
    let walked = walk(index, client, base, pace).await?;
    index.catalog_meta_set(META_SEQ, &seq).await?;
    Ok(walked + changes(index, client, base, pace, seq).await?)
}

async fn walk(
    index: &CacheBackend,
    client: &impl Service<Request, Output = Response, Error = OpaqueError>,
    base: &str,
    pace: Duration,
) -> Result<usize> {
    let mut synced = 0;
    let mut after = index.catalog_meta_get(META_WALK).await?;
    loop {
        // `startkey` is inclusive, and npm's replica refuses CouchDB's
        // `skip` (400): the page repeats the cursor row, dropped below.
        let url = match &after {
            Some(after) => {
                let key = urlencode(&serde_json::to_string(after)?);
                format!("{base}/_all_docs?limit={PAGE}&startkey={key}")
            }
            None => format!("{base}/_all_docs?limit={PAGE}"),
        };
        let page: AllDocs = get(client, &url).await?;
        let full = page.rows.len() == PAGE;
        // The cursor is the last row, whether or not it is a package.
        let last = page.rows.last().map(|row| row.id.clone());
        let names: Vec<CatalogName> = page
            .rows
            .into_iter()
            .map(|row| row.id)
            .filter(|id| after.as_ref() != Some(id))
            // Design documents are CouchDB's, not packages.
            .filter(|id| !id.starts_with('_'))
            .map(|name| CatalogName {
                name,
                latest_version: None,
            })
            .collect();
        index.names_upsert(AssetKind::NpmPackage, &names).await?;
        synced += names.len();
        if let Some(last) = last {
            index.catalog_meta_set(META_WALK, &last).await?;
            after = Some(last);
        }
        if !full {
            return Ok(synced);
        }
        tokio::time::sleep(pace).await;
    }
}

async fn changes(
    index: &CacheBackend,
    client: &impl Service<Request, Output = Response, Error = OpaqueError>,
    base: &str,
    pace: Duration,
    mut since: String,
) -> Result<usize> {
    let mut synced = 0;
    loop {
        let url = format!("{base}/_changes?since={}&limit={PAGE}", urlencode(&since));
        let page: Changes = get(client, &url).await?;
        if page.results.is_empty() {
            return Ok(synced);
        }
        let (gone, live): (Vec<Change>, Vec<Change>) = page
            .results
            .into_iter()
            .filter(|change| !change.id.starts_with('_'))
            .partition(|change| change.deleted);
        let live: Vec<CatalogName> = live
            .into_iter()
            .map(|change| CatalogName {
                name: change.id,
                latest_version: None,
            })
            .collect();
        let gone: Vec<String> = gone.into_iter().map(|change| change.id).collect();
        index.names_upsert(AssetKind::NpmPackage, &live).await?;
        index.names_remove(AssetKind::NpmPackage, &gone).await?;
        synced += live.len() + gone.len();
        since = sequence(&page.last_seq);
        index.catalog_meta_set(META_SEQ, &since).await?;
        tokio::time::sleep(pace).await;
    }
}

async fn get<T: serde::de::DeserializeOwned + Send + 'static>(
    client: &impl Service<Request, Output = Response, Error = OpaqueError>,
    url: &str,
) -> Result<T> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|err| anyhow!("asking npm's replica for {url}: {err}"))?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "npm's replica answered {} for {url}",
            response.status()
        ));
    }
    response
        .try_into_json::<T>()
        .await
        .map_err(|err| anyhow!("reading npm's replica {url}: {err}"))
}

/// A `CouchDB` sequence: a number (npm's replica) or an opaque string.
fn sequence(seq: &Value) -> String {
    seq.as_str()
        .map_or_else(|| seq.to_string(), ToOwned::to_owned)
}

fn urlencode(value: &str) -> String {
    percent_encoding::utf8_percent_encode(value, percent_encoding::NON_ALPHANUMERIC).to_string()
}

#[cfg(test)]
mod tests;
