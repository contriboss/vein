//! rubygems.org's catalog: `names.gz`, every gem name, fetched whole when
//! it changed (`ETag` / `Last-Modified`).

use anyhow::Result;
use rama::{
    Service,
    error::extra::OpaqueError,
    http::{
        BodyExtractExt, Request, Response, StatusCode, header, service::client::HttpClientExt as _,
    },
    telemetry::tracing::info,
};
use vein_adapter::{CacheBackend, CacheBackendTrait};

const NAMES_URL: &str = "https://rubygems.org/names.gz";
const META_ETAG: &str = "catalog_names_etag";
const META_LAST_MODIFIED: &str = "catalog_names_last_modified";

/// Names synced, or `None` when the list had not changed.
pub async fn sync(
    index: &CacheBackend,
    client: &impl Service<Request, Output = Response, Error = OpaqueError>,
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
        .map_err(|e| anyhow::anyhow!("requesting rubygems names list: {e}"))?;

    if response.status() == StatusCode::NOT_MODIFIED {
        info!("catalog names list is up to date");
        return Ok(None);
    }

    if !response.status().is_success() {
        return Err(anyhow::anyhow!("fetching rubygems names list"));
    }

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

    let text = response
        .try_into_string()
        .await
        .map_err(|e| anyhow::anyhow!("decoding names list: {e}"))?;

    let names: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(std::borrow::ToOwned::to_owned)
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
