use std::time::Duration;

use anyhow::{Context, Result};
use rama::{
    Layer, Service,
    error::OpaqueError,
    http::{
        BodyExtractExt, HeaderValue, Request, Response, StatusCode, client::EasyHttpWebClient,
        header, layer::required_header::AddRequiredRequestHeadersLayer,
        service::client::HttpClientExt as _,
    },
    layer::{MapErrLayer, TimeoutLayer},
    telemetry::tracing::info,
};
use vein_adapter::CacheBackendKind;

const NAMES_URL: &str = "https://rubygems.org/names.gz";
const META_ETAG: &str = "catalog_names_etag";
const META_LAST_MODIFIED: &str = "catalog_names_last_modified";

pub async fn sync_names_once(index: &CacheBackendKind) -> Result<Option<usize>> {
    let client = build_client()?;
    sync_names_with_client(index, &client).await
}

fn build_client() -> Result<impl Service<Request, Output = Response, Error = OpaqueError>> {
    // NOTE if you want pooling you'll have to
    // use build_connector to also include the pool desired
    let inner = EasyHttpWebClient::default();

    // decompression support would be added as layer on top

    Ok((
        MapErrLayer::new(OpaqueError::from_boxed),
        TimeoutLayer::new(Duration::from_secs(60)),
        AddRequiredRequestHeadersLayer::new()
            .with_user_agent_header_value(HeaderValue::from_static("vein-catalog/0.1.0")),
    )
        .into_layer(inner))
}

async fn sync_names_with_client(
    index: &CacheBackendKind,
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
        .context("requesting rubygems names list")?;

    if response.status() == StatusCode::NOT_MODIFIED {
        info!("catalog names list is up to date");
        return Ok(None);
    }

    if !response.status().is_success() {
        return Err(anyhow::anyhow!("fetching rubygems names list"));
    };

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
        .context("decoding names list")?;

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
