//! crates.io's catalog, from its API: one full crawl in name order (resumed
//! where it stopped if vein restarts), then the crates updated since the
//! last sync. Paced at one request a second, crates.io's crawler policy.

use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use jiff::Timestamp;
use rama::{
    Service,
    error::extra::OpaqueError,
    http::{BodyExtractExt, Request, Response, service::client::HttpClientExt as _},
};
use serde::Deserialize;
use vein_adapter::{AssetKind, CacheBackend, CacheBackendTrait, CatalogName};

pub const API: &str = "https://crates.io/api/v1/crates";
/// The query of the next page of the full crawl; `done` once it finished.
const META_CRAWL: &str = "catalog_crates_crawl";
/// When the last sync started: the next one asks for crates updated since.
const META_SINCE: &str = "catalog_crates_since";
const FIRST_PAGE: &str = "?per_page=100&sort=alpha";
const UPDATES: &str = "?per_page=100&sort=recent-updates";
const DONE: &str = "done";

#[derive(Deserialize)]
struct Page {
    crates: Vec<Crate>,
    meta: Meta,
}

#[derive(Deserialize)]
struct Crate {
    name: String,
    max_stable_version: Option<String>,
    max_version: Option<String>,
    updated_at: Timestamp,
}

#[derive(Deserialize)]
struct Meta {
    next_page: Option<String>,
}

/// Names synced.
pub async fn sync(
    index: &CacheBackend,
    client: &impl Service<Request, Output = Response, Error = OpaqueError>,
    api: &str,
    pace: Duration,
) -> Result<usize> {
    let started = Timestamp::now();
    let crawl = index.catalog_meta_get(META_CRAWL).await?;
    let synced = if crawl.as_deref() == Some(DONE) {
        let since = match index.catalog_meta_get(META_SINCE).await? {
            Some(since) => since.parse().context("reading the last crates sync time")?,
            None => Timestamp::UNIX_EPOCH,
        };
        updates(index, client, api, pace, since).await?
    } else {
        crawl_all(
            index,
            client,
            api,
            pace,
            crawl.as_deref().unwrap_or(FIRST_PAGE),
        )
        .await?
    };
    index
        .catalog_meta_set(META_SINCE, &started.to_string())
        .await?;
    Ok(synced)
}

async fn crawl_all(
    index: &CacheBackend,
    client: &impl Service<Request, Output = Response, Error = OpaqueError>,
    api: &str,
    pace: Duration,
    from: &str,
) -> Result<usize> {
    let mut synced = 0;
    let mut query = from.to_owned();
    loop {
        let page = fetch(client, api, &query).await?;
        synced += store(index, &page.crates).await?;
        let Some(next) = page.meta.next_page.filter(|_| !page.crates.is_empty()) else {
            index.catalog_meta_set(META_CRAWL, DONE).await?;
            return Ok(synced);
        };
        // Saved before the pause: a restart resumes on the next page.
        index.catalog_meta_set(META_CRAWL, &next).await?;
        query = next;
        tokio::time::sleep(pace).await;
    }
}

/// Crates updated since `since`, newest update first, until the pages
/// reach older ones.
async fn updates(
    index: &CacheBackend,
    client: &impl Service<Request, Output = Response, Error = OpaqueError>,
    api: &str,
    pace: Duration,
    since: Timestamp,
) -> Result<usize> {
    let mut synced = 0;
    let mut query = UPDATES.to_owned();
    loop {
        let page = fetch(client, api, &query).await?;
        let fresh: Vec<Crate> = page
            .crates
            .into_iter()
            .take_while(|c| c.updated_at >= since)
            .collect();
        let reached_older = fresh.len() < 100;
        synced += store(index, &fresh).await?;
        match page.meta.next_page {
            Some(next) if !reached_older => query = next,
            _ => return Ok(synced),
        }
        tokio::time::sleep(pace).await;
    }
}

async fn fetch(
    client: &impl Service<Request, Output = Response, Error = OpaqueError>,
    api: &str,
    query: &str,
) -> Result<Page> {
    let url = format!("{api}{query}");
    let response = client
        .get(url.as_str())
        .send()
        .await
        .map_err(|err| anyhow!("asking crates.io for {query}: {err}"))?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "crates.io answered {} for {query}",
            response.status()
        ));
    }
    response
        .try_into_json::<Page>()
        .await
        .map_err(|err| anyhow!("reading crates.io's page {query}: {err}"))
}

async fn store(index: &CacheBackend, crates: &[Crate]) -> Result<usize> {
    let names: Vec<CatalogName> = crates
        .iter()
        .map(|c| CatalogName {
            name: c.name.clone(),
            latest_version: c
                .max_stable_version
                .clone()
                .or_else(|| c.max_version.clone()),
        })
        .collect();
    index.names_upsert(AssetKind::Crate, &names).await?;
    Ok(names.len())
}

#[cfg(test)]
mod tests;
