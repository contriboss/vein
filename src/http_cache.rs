use std::future::Future;

use anyhow::{Context, Result};
use rama::http::{Body, HeaderMap, HeaderValue, Response, StatusCode, header};
use tokio::io::AsyncWriteExt;
use vein_adapter::{CacheBackend, CacheBackendTrait, FilesystemStorage};

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CacheEntryMeta {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

impl CacheEntryMeta {
    pub fn from_headers(headers: &HeaderMap) -> Self {
        Self {
            etag: headers
                .get(header::ETAG)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string()),
            last_modified: headers
                .get(header::LAST_MODIFIED)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaStoreMode {
    Strict,
    BestEffort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheOutcome {
    Hit,
    Miss,
    Revalidated,
    Pass,
}

pub struct CachedFetchResult {
    pub response: Response<Body>,
    pub outcome: CacheOutcome,
}

pub async fn fetch_cached_text<F, Fut, T, TFut>(
    storage: &FilesystemStorage,
    index: &CacheBackend,
    storage_path: &str,
    meta_key: &str,
    content_type: &str,
    cache_control: &str,
    include_content_length: bool,
    meta_mode: MetaStoreMode,
    strip_transfer_encoding: bool,
    fetch: F,
    transform: T,
) -> Result<CachedFetchResult>
where
    F: FnOnce(HeaderMap) -> Fut,
    Fut: Future<Output = Result<Response<Body>>>,
    T: FnOnce(Vec<u8>) -> TFut,
    TFut: Future<Output = Result<Vec<u8>>>,
{
    let cached_bytes = tokio::fs::read(storage.resolve(storage_path)).await.ok();
    let had_cache = cached_bytes.is_some();

    let cached_meta = load_cached_meta(index, meta_key, meta_mode).await?;
    let request_headers = build_conditional_headers(&cached_meta);

    let response = fetch(request_headers).await?;
    let status = response.status();

    if status == StatusCode::NOT_MODIFIED && cached_bytes.is_some() {
        let body = cached_bytes.unwrap_or_default();
        let meta = cached_meta.unwrap_or_default();
        let transformed = transform(body).await?;
        let response = build_cached_response(
            &transformed,
            &meta,
            content_type,
            cache_control,
            include_content_length,
        )?;
        return Ok(CachedFetchResult {
            response,
            outcome: CacheOutcome::Revalidated,
        });
    }

    if status.is_success() {
        use rama::http::body::util::BodyExt;
        let headers = response.headers().clone();
        let body = response
            .into_body()
            .collect()
            .await
            .context("reading cached response body")?
            .to_bytes();
        let meta = CacheEntryMeta::from_headers(&headers);

        persist_body(storage, storage_path, &body).await?;
        store_cached_meta(index, meta_key, &meta, meta_mode).await?;

        let transformed = transform(body.to_vec()).await?;
        let response = build_cached_response(
            &transformed,
            &meta,
            content_type,
            cache_control,
            include_content_length,
        )?;
        let outcome = if had_cache {
            CacheOutcome::Revalidated
        } else {
            CacheOutcome::Miss
        };
        return Ok(CachedFetchResult { response, outcome });
    }

    let forwarded = forward_response(response, strip_transfer_encoding).await?;
    Ok(CachedFetchResult {
        response: forwarded,
        outcome: CacheOutcome::Pass,
    })
}

fn build_conditional_headers(meta: &Option<CacheEntryMeta>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Some(meta) = meta {
        if let Some(etag) = &meta.etag
            && let Ok(value) = HeaderValue::from_str(etag)
        {
            headers.insert(header::IF_NONE_MATCH, value);
        }
        if let Some(last_modified) = &meta.last_modified
            && let Ok(value) = HeaderValue::from_str(last_modified)
        {
            headers.insert(header::IF_MODIFIED_SINCE, value);
        }
    }
    headers
}

async fn load_cached_meta(
    index: &CacheBackend,
    meta_key: &str,
    mode: MetaStoreMode,
) -> Result<Option<CacheEntryMeta>> {
    let raw = match index.catalog_meta_get(meta_key).await {
        Ok(value) => value,
        Err(err) => {
            if mode == MetaStoreMode::Strict {
                return Err(err).context("loading cached metadata");
            }
            return Ok(None);
        }
    };
    Ok(raw.and_then(|value| serde_json::from_str(&value).ok()))
}

async fn store_cached_meta(
    index: &CacheBackend,
    meta_key: &str,
    meta: &CacheEntryMeta,
    mode: MetaStoreMode,
) -> Result<()> {
    let meta_json = match serde_json::to_string(meta) {
        Ok(value) => value,
        Err(err) => {
            if mode == MetaStoreMode::Strict {
                return Err(err).context("serializing cache metadata");
            }
            return Ok(());
        }
    };
    match index.catalog_meta_set(meta_key, &meta_json).await {
        Ok(()) => Ok(()),
        Err(err) => {
            if mode == MetaStoreMode::Strict {
                Err(err).context("persisting cache metadata")
            } else {
                Ok(())
            }
        }
    }
}

async fn persist_body(storage: &FilesystemStorage, storage_path: &str, body: &[u8]) -> Result<()> {
    let mut temp = storage
        .create_temp_writer(storage_path)
        .await
        .context("creating cache temp file")?;
    temp.file_mut()
        .write_all(body)
        .await
        .context("writing cache body")?;
    temp.commit().await.context("committing cache body")?;
    Ok(())
}

fn build_cached_response(
    body: &[u8],
    meta: &CacheEntryMeta,
    content_type: &str,
    cache_control: &str,
    include_content_length: bool,
) -> Result<Response<Body>> {
    let mut builder = Response::builder().status(StatusCode::OK);
    {
        let headers = builder.headers_mut().context("getting headers")?;
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_str(content_type)?);
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_str(cache_control)?,
        );
        if include_content_length {
            headers.insert(
                header::CONTENT_LENGTH,
                HeaderValue::from_str(&body.len().to_string())?,
            );
        }
        if let Some(etag) = &meta.etag
            && let Ok(value) = HeaderValue::from_str(etag)
        {
            headers.insert(header::ETAG, value);
        }
        if let Some(last_modified) = &meta.last_modified
            && let Ok(value) = HeaderValue::from_str(last_modified)
        {
            headers.insert(header::LAST_MODIFIED, value);
        }
    }
    builder
        .body(Body::from(body.to_vec()))
        .context("building cached response")
}

async fn forward_response(
    response: Response<Body>,
    strip_transfer_encoding: bool,
) -> Result<Response<Body>> {
    let status = response.status();
    let headers = response.headers().clone();

    use rama::http::body::util::BodyExt;
    let body_bytes = response
        .into_body()
        .collect()
        .await
        .context("reading forwarded response body")?
        .to_bytes();

    let mut builder = Response::builder().status(status);
    {
        let resp_headers = builder.headers_mut().context("getting headers")?;
        for (name, value) in headers.iter() {
            if strip_transfer_encoding && name == header::TRANSFER_ENCODING {
                continue;
            }
            resp_headers.insert(name, value.clone());
        }
    }

    builder
        .body(Body::from(body_bytes))
        .context("building forwarded response")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_conditional_headers_sets_etag_and_last_modified() {
        let meta = CacheEntryMeta {
            etag: Some("\"abc\"".to_string()),
            last_modified: Some("Wed, 01 Jan 2025 00:00:00 GMT".to_string()),
        };
        let headers = build_conditional_headers(&Some(meta));
        assert_eq!(
            headers.get(header::IF_NONE_MATCH).unwrap(),
            "\"abc\""
        );
        assert_eq!(
            headers.get(header::IF_MODIFIED_SINCE).unwrap(),
            "Wed, 01 Jan 2025 00:00:00 GMT"
        );
    }

    #[test]
    fn build_conditional_headers_empty_without_meta() {
        let headers = build_conditional_headers(&None);
        assert!(headers.is_empty());
    }
}
