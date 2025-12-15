use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use reqwest::{Client, StatusCode, header};
use tokio::time::sleep;
use tracing::{error, info};
use vein_adapter::CacheBackend;

const NAMES_URL: &str = "https://rubygems.org/names.gz";
const META_ETAG: &str = "catalog_names_etag";
const META_LAST_MODIFIED: &str = "catalog_names_last_modified";
const SYNC_INTERVAL: Duration = Duration::from_secs(30 * 60);

pub fn spawn_background_sync(index: Arc<dyn CacheBackend>) -> Result<()> {
    let client = build_client()?;
    tokio::spawn(async move {
        if let Err(err) = sync_loop(index, client).await {
            error!(error = %err, "catalog sync loop terminated");
        }
    });
    Ok(())
}

async fn sync_loop(index: Arc<dyn CacheBackend>, client: Client) -> Result<()> {
    if let Err(err) = sync_names_with_client(index.as_ref(), &client).await {
        error!(error = %err, "initial catalog sync failed");
    }
    loop {
        sleep(SYNC_INTERVAL).await;
        if let Err(err) = sync_names_with_client(index.as_ref(), &client).await {
            error!(error = %err, "catalog sync failed");
        }
    }
}

pub async fn sync_names_once(index: &dyn CacheBackend) -> Result<Option<usize>> {
    let client = build_client()?;
    sync_names_with_client(index, &client).await
}

fn build_client() -> Result<Client> {
    Client::builder()
        .user_agent("vein-catalog/0.1.0")
        .no_gzip()
        .no_deflate()
        .no_brotli()
        .timeout(Duration::from_secs(60))
        .build()
        .context("building catalog HTTP client")
}

async fn sync_names_with_client(
    index: &dyn CacheBackend,
    client: &Client,
) -> Result<Option<usize>> {
    let mut request = client.get(NAMES_URL);

    if let Some(etag) = index.catalog_meta_get(META_ETAG).await? {
        request = request.header(header::IF_NONE_MATCH, etag);
    }

    if let Some(last_modified) = index.catalog_meta_get(META_LAST_MODIFIED).await? {
        request = request.header(header::IF_MODIFIED_SINCE, last_modified);
    }

    let response = request
        .send()
        .await
        .context("requesting rubygems names list")?;

    if response.status() == StatusCode::NOT_MODIFIED {
        info!("catalog names list is up to date");
        return Ok(None);
    }

    let response = response
        .error_for_status()
        .context("fetching rubygems names list")?;

    let etag = response
        .headers()
        .get(header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let last_modified = response
        .headers()
        .get(header::LAST_MODIFIED)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);

    let bytes = response.bytes().await?.to_vec();
    let text = String::from_utf8(bytes).context("decoding names list")?;
    let names: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.to_owned())
        .collect();

    let total = names.len();
    for chunk in names.chunks(1_000) {
        index.catalog_upsert_names(chunk).await?;
    }

    if let Some(etag) = etag {
        index.catalog_meta_set(META_ETAG, &etag).await?;
    }
    if let Some(last_modified) = last_modified {
        index
            .catalog_meta_set(META_LAST_MODIFIED, &last_modified)
            .await?;
    }

    info!(total, "catalog names synced");

    Ok(Some(total))
}
