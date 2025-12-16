mod cache;
mod handlers;
mod quarantine;
mod response;
mod types;
mod utils;

use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use http::header;
use percent_encoding::percent_decode_str;
use rama::{
    Service,
    error::BoxError,
    http::{
        Body, HeaderMap, HeaderValue, Method, Request, Response, StatusCode, body::util::BodyExt,
    },
};
use tokio::io::AsyncWriteExt;
use tracing::{error, info, warn};

use crate::{config::Config, upstream::UpstreamClient};
use vein_adapter::{CacheBackend, FilesystemStorage};

// Re-export public types
pub use types::{CacheStatus, RequestContext, UpstreamTarget};

#[derive(Debug, Clone)]
pub enum CompactRequest {
    Versions,
    Names,
    Info { name: String },
}

impl CompactRequest {
    fn from_path(path: &str) -> Option<Self> {
        match path {
            "/versions" => Some(Self::Versions),
            "/names" => Some(Self::Names),
            _ if path.starts_with("/info/") => {
                let decoded = path.trim_start_matches("/info/");
                if decoded.is_empty() {
                    return None;
                }
                let decoded = percent_decode_str(decoded).decode_utf8().ok()?;
                Some(Self::Info {
                    name: decoded.to_string(),
                })
            }
            _ => None,
        }
    }

    fn storage_path(&self) -> String {
        match self {
            Self::Versions => "compact_index/versions".to_string(),
            Self::Names => "compact_index/names".to_string(),
            Self::Info { name, .. } => {
                format!("compact_index/info/{}", utils::sanitize_filename(name))
            }
        }
    }

    fn meta_key(&self) -> String {
        match self {
            Self::Versions => "compact:versions".to_string(),
            Self::Names => "compact:names".to_string(),
            Self::Info { name, .. } => format!("compact:info:{name}"),
        }
    }

    fn content_type(&self) -> &'static str {
        "text/plain"
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct CompactEntryMeta {
    etag: Option<String>,
    last_modified: Option<String>,
}

impl CompactEntryMeta {
    fn from_headers(headers: &reqwest::header::HeaderMap) -> Self {
        Self {
            etag: headers
                .get(reqwest::header::ETAG)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string()),
            last_modified: headers
                .get(reqwest::header::LAST_MODIFIED)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string()),
        }
    }
}

/// Main proxy service
#[derive(Clone)]
pub struct VeinProxy {
    config: Arc<Config>,
    storage: Arc<FilesystemStorage>,
    index: Arc<dyn CacheBackend>,
    upstreams: Vec<UpstreamTarget>,
    upstream_client: Option<UpstreamClient>,
}

impl VeinProxy {
    pub fn new(
        config: Arc<Config>,
        storage: Arc<FilesystemStorage>,
        index: Arc<dyn CacheBackend>,
    ) -> Result<Self> {
        let (upstreams, upstream_client) = if let Some(ref upstream_config) = config.upstream {
            let mut upstreams = Vec::new();
            upstreams.push(UpstreamTarget::from_url(&upstream_config.url)?);
            for url in &upstream_config.fallback_urls {
                upstreams.push(UpstreamTarget::from_url(url)?);
            }
            let client =
                UpstreamClient::new(upstream_config).context("building upstream client")?;
            (upstreams, Some(client))
        } else {
            tracing::info!("No upstream configured - running in cache-only mode");
            (Vec::new(), None)
        };

        Ok(Self {
            config,
            storage,
            index,
            upstreams,
            upstream_client,
        })
    }

    async fn handle(&self, req: Request<Body>, ctx: &mut RequestContext) -> Result<Response<Body>> {
        let method = req.method().clone();
        let path = req.uri().path().to_owned();

        if method == Method::GET {
            match path.as_str() {
                "/up" => {
                    let (resp, status) =
                        handlers::handle_health(self.index.as_ref()).await?;
                    ctx.cache = status;
                    return Ok(resp);
                }
                "/.well-known/vein/sbom" => {
                    let (resp, status) =
                        handlers::handle_sbom_request(&req, self.index.as_ref()).await?;
                    ctx.cache = status;
                    return Ok(resp);
                }
                "/" => {
                    ctx.cache = CacheStatus::Pass;
                    return response::respond_homepage(&self.config);
                }
                _ => {}
            }
        }

        match self.try_handle_cached_request(&req, ctx).await? {
            Some((cache_status, resp)) => {
                ctx.cache = cache_status;
                return Ok(resp);
            }
            None => {
                // Cache miss - try upstream if configured
                if self.upstreams.is_empty() {
                    ctx.cache = CacheStatus::Miss;
                    return response::respond_text(StatusCode::NOT_FOUND, "not found in cache");
                }

                if method == Method::GET {
                    if let Some(compact) = CompactRequest::from_path(path.as_str()) {
                        match self.handle_compact_request(&req, compact, ctx).await {
                            Ok(Some((resp, status))) => {
                                ctx.cache = status;
                                return Ok(resp);
                            }
                            Ok(None) => {}
                            Err(err) => {
                                ctx.cache = CacheStatus::Error;
                                error!(error = %err, "failed to serve compact index request");
                                return response::respond_text(
                                    StatusCode::BAD_GATEWAY,
                                    "upstream error",
                                );
                            }
                        }
                    }

                    match self.proxy_generic_get(&req).await {
                        Ok(resp) => {
                            ctx.cache = CacheStatus::Pass;
                            return Ok(resp);
                        }
                        Err(err) => {
                            ctx.cache = CacheStatus::Error;
                            error!(
                                error = %err,
                                summary = %self.request_summary(ctx),
                                "failed to proxy request to upstream"
                            );
                            return response::respond_text(
                                StatusCode::BAD_GATEWAY,
                                "upstream error",
                            );
                        }
                    }
                }
                ctx.cache = CacheStatus::Pass;
            }
        }

        response::respond_text(StatusCode::BAD_REQUEST, "unsupported request")
    }

    async fn try_handle_cached_request(
        &self,
        req: &Request<Body>,
        ctx: &mut RequestContext,
    ) -> Result<Option<(CacheStatus, Response<Body>)>> {
        use types::CacheableRequest;

        ctx.method = req.method().clone();
        ctx.path = req.uri().path().to_string();

        if req.method() != Method::GET {
            return Ok(None);
        }

        let Some(cacheable) = CacheableRequest::from_request(req) else {
            return Ok(None);
        };

        match self.index.get(&cacheable.asset_key()).await? {
            Some(entry) => {
                match cache::serve_cached(&cacheable, entry, &self.storage).await {
                    Ok(resp) => Ok(Some((CacheStatus::Hit, resp))),
                    Err(err) => {
                        warn!(
                            error = %err,
                            "failed to serve cached asset, falling back to upstream"
                        );
                        // treat as miss, refetch from upstream
                        self.fetch_and_stream(req, &cacheable, true)
                            .await
                            .map(|resp| Some((CacheStatus::Revalidated, resp)))
                    }
                }
            }
            None => self
                .fetch_and_stream(req, &cacheable, false)
                .await
                .map(|resp| Some((CacheStatus::Miss, resp))),
        }
    }

    async fn handle_compact_request(
        &self,
        req: &Request<Body>,
        compact: CompactRequest,
        ctx: &mut RequestContext,
    ) -> Result<Option<(Response<Body>, CacheStatus)>> {
        let storage_path = compact.storage_path();
        let cached_bytes = tokio::fs::read(self.storage.resolve(&storage_path))
            .await
            .ok();

        let meta_key = compact.meta_key();
        let cached_meta: Option<CompactEntryMeta> = self
            .index
            .catalog_meta_get(&meta_key)
            .await?
            .and_then(|raw| serde_json::from_str(&raw).ok());

        let mut headers = HeaderMap::new();
        if let Some(meta) = &cached_meta {
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

        let response = self
            .fetch_with_fallback(req, Some(&headers))
            .await
            .context("requesting compact index")?;

        let status = response.status();

        if status == reqwest::StatusCode::NOT_MODIFIED && cached_bytes.is_some() {
            let meta = cached_meta.unwrap_or_default();
            let body = cached_bytes.as_deref().unwrap_or_default();

            // Apply quarantine filtering for /info/{gem} requests
            let filtered_body = match &compact {
                CompactRequest::Info { name } => {
                    quarantine::filter_compact_info(
                        &self.config.delay_policy,
                        self.index.as_ref(),
                        name,
                        body,
                    )
                    .await
                    .unwrap_or_else(|err| {
                        warn!(error = %err, gem = %name, "Failed to filter quarantined versions");
                        body.to_vec()
                    })
                }
                _ => body.to_vec(),
            };

            let resp = self.write_compact_response(&filtered_body, &meta, compact.content_type())?;
            ctx.cache = CacheStatus::Revalidated;
            return Ok(Some((resp, CacheStatus::Revalidated)));
            // If we got a 304 but have no cache, fall through to refetch below
        }

        if status.is_success() {
            let headers = response.headers().clone();
            let body = response
                .into_body()
                .collect()
                .await
                .context("reading compact index body")?
                .to_bytes();
            let meta = CompactEntryMeta::from_headers(&headers);

            let mut temp = self
                .storage
                .create_temp_writer(&storage_path)
                .await
                .context("creating compact index temp file")?;
            temp.file_mut()
                .write_all(&body)
                .await
                .context("writing compact index body")?;
            temp.commit()
                .await
                .context("committing compact index body")?;

            let meta_json = serde_json::to_string(&meta).context("serializing compact meta")?;
            self.index
                .catalog_meta_set(&meta_key, &meta_json)
                .await
                .context("persisting compact meta")?;

            // Apply quarantine filtering for /info/{gem} requests
            let filtered_body = match &compact {
                CompactRequest::Info { name } => {
                    quarantine::filter_compact_info(
                        &self.config.delay_policy,
                        self.index.as_ref(),
                        name,
                        &body,
                    )
                    .await
                    .unwrap_or_else(|err| {
                        warn!(error = %err, gem = %name, "Failed to filter quarantined versions");
                        body.to_vec()
                    })
                }
                _ => body.to_vec(),
            };

            let resp = self.write_compact_response(&filtered_body, &meta, compact.content_type())?;
            let cache_status = if cached_bytes.is_some() {
                CacheStatus::Revalidated
            } else {
                CacheStatus::Miss
            };
            ctx.cache = cache_status;
            return Ok(Some((resp, cache_status)));
        }

        // Propagate non-success upstream status to client
        ctx.cache = CacheStatus::Pass;
        let resp = self.forward_response(response).await?;
        Ok(Some((resp, CacheStatus::Pass)))
    }

    fn write_compact_response(
        &self,
        body: &[u8],
        meta: &CompactEntryMeta,
        content_type: &str,
    ) -> Result<Response<Body>> {
        let mut builder = Response::builder().status(StatusCode::OK);
        let headers = builder.headers_mut().expect("headers mut");
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_str(content_type)?);
        headers.insert(
            header::CONTENT_LENGTH,
            HeaderValue::from_str(&body.len().to_string())?,
        );
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=300"),
        );
        if let Some(etag) = &meta.etag {
            headers.insert(header::ETAG, HeaderValue::from_str(etag)?);
        }
        if let Some(last_modified) = &meta.last_modified {
            headers.insert(header::LAST_MODIFIED, HeaderValue::from_str(last_modified)?);
        }

        builder
            .body(Body::from(body.to_vec()))
            .context("building compact response")
    }

    async fn fetch_and_stream(
        &self,
        req: &Request<Body>,
        cacheable: &types::CacheableRequest,
        treating_as_revalidation: bool,
    ) -> Result<Response<Body>> {
        let response = self
            .fetch_with_fallback(req, None)
            .await
            .context("requesting upstream")?;

        if !response.status().is_success() {
            warn!(
                status = %response.status(),
                path = %req.uri().path(),
                "upstream returned error status"
            );
            return self.forward_response(response).await;
        }

        let temp_file = self
            .storage
            .create_temp_writer(&cacheable.relative_path)
            .await
            .context("creating temp file")?;

        let result = cache::run_cache_miss_flow(
            cacheable,
            self.index.clone(),
            self.storage.clone(),
            response,
            temp_file,
            treating_as_revalidation,
        )
        .await;

        // Record new version in quarantine system (only for gems, not specs)
        if result.is_ok() && cacheable.kind == vein_adapter::AssetKind::Gem
            && let Err(err) = quarantine::record_new_version(
                &self.config.delay_policy,
                self.index.as_ref(),
                &cacheable.name,
                &cacheable.version,
                cacheable.platform.as_deref(),
                "", // SHA256 is computed in run_cache_miss_flow, not available here
            )
            .await
            {
                warn!(
                    error = %err,
                    gem = %cacheable.name,
                    version = %cacheable.version,
                    "Failed to record version in quarantine system"
                );
            }

        result
    }

    fn request_summary(&self, ctx: &RequestContext) -> String {
        format!("{} {}", ctx.method.as_str(), ctx.path)
    }

    async fn forward_response(&self, response: Response<Body>) -> Result<Response<Body>> {
        let status = response.status();
        let mut builder = Response::builder().status(status);
        {
            let headers = builder
                .headers_mut()
                .ok_or_else(|| anyhow!("failed to get headers for response build"))?;
            for (name, value) in response.headers().iter() {
                if name == header::TRANSFER_ENCODING {
                    continue;
                }
                headers.insert(name, value.clone());
            }
        }

        let body = Body::from(
            response
                .into_body()
                .collect()
                .await
                .context("reading forwarded response body")?
                .to_bytes(),
        );
        builder.body(body).context("building forwarded response")
    }

    async fn proxy_generic_get(&self, req: &Request<Body>) -> Result<Response<Body>> {
        let response = self
            .fetch_with_fallback(req, None)
            .await
            .context("requesting upstream")?;
        self.forward_response(response).await
    }

    async fn fetch_with_fallback(
        &self,
        req: &Request<Body>,
        headers: Option<&HeaderMap>,
    ) -> Result<Response<Body>> {
        let client = self
            .upstream_client
            .as_ref()
            .ok_or_else(|| anyhow!("upstream client not configured"))?;

        if self.upstreams.is_empty() {
            return Err(anyhow!("no upstream configured"));
        }

        let mut last_err: Option<anyhow::Error> = None;

        for upstream in &self.upstreams {
            let upstream_url = upstream
                .join(req)
                .with_context(|| format!("constructing upstream url for {}", upstream.base))?;

            let resp = client
                .get_with_headers(upstream_url.as_str(), headers.unwrap_or(&HeaderMap::new()))
                .await;

            match resp {
                Ok(r) if r.status().is_server_error() => {
                    last_err = Some(anyhow!("{} returned {}", upstream_url, r.status()));
                    continue;
                }
                Ok(r) => return Ok(r),
                Err(err) => {
                    last_err = Some(anyhow!("{} failed: {err}", upstream_url));
                    continue;
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow!("all upstreams failed")))
    }
}

impl Service<Request<Body>> for VeinProxy {
    type Output = Response<Body>;
    type Error = BoxError;

    async fn serve(&self, req: Request<Body>) -> Result<Self::Output, Self::Error> {
        use std::time::Instant;
        let mut ctx = RequestContext::from_request(&req);
        ctx.start = Instant::now();

        let result = self.handle(req, &mut ctx).await;

        match &result {
            Ok(resp) => {
                let response_code = resp.status().as_u16();
                let duration_ms = ctx.start.elapsed().as_millis();
                info!(
                    summary = %self.request_summary(&ctx),
                    response_code,
                    duration_ms,
                    cache_status = %ctx.cache,
                    "request handled"
                );
            }
            Err(err) => {
                let duration_ms = ctx.start.elapsed().as_millis();
                error!(
                    summary = %self.request_summary(&ctx),
                    duration_ms,
                    cache_status = %ctx.cache,
                    error = %err,
                    "request failed"
                );
            }
        }

        result.map_err(|e| e.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use types::{CacheStatus, CacheableRequest, UpstreamTarget};
    use vein_adapter::AssetKind;

    fn req(path: &str) -> Request<Body> {
        Request::builder()
            .method(Method::GET)
            .uri(path)
            .body(Body::empty())
            .unwrap()
    }

    // ============================================================================
    // CacheableRequest::from_gem_path tests
    // ============================================================================

    #[test]
    fn from_gem_path_simple() {
        let result = CacheableRequest::from_gem_path("rack-3.0.0.gem").unwrap();
        assert_eq!(result.kind, AssetKind::Gem);
        assert_eq!(result.name, "rack");
        assert_eq!(result.version, "3.0.0");
        assert!(result.platform.is_none());
        assert_eq!(result.file_name, "rack-3.0.0.gem");
        assert_eq!(result.relative_path, "gems/rack/rack-3.0.0.gem");
    }

    #[test]
    fn from_gem_path_with_platform() {
        let result = CacheableRequest::from_gem_path("nokogiri-1.15.5-x86_64-darwin.gem").unwrap();
        assert_eq!(result.kind, AssetKind::Gem);
        assert_eq!(result.name, "nokogiri");
        assert_eq!(result.version, "1.15.5");
        assert_eq!(result.platform.as_deref(), Some("x86_64-darwin"));
        assert_eq!(result.file_name, "nokogiri-1.15.5-x86_64-darwin.gem");
        assert_eq!(
            result.relative_path,
            "gems/nokogiri/nokogiri-1.15.5-x86_64-darwin.gem"
        );
    }

    #[test]
    fn from_gem_path_hyphenated_name() {
        let result = CacheableRequest::from_gem_path("active-support-7.1.0.gem").unwrap();
        assert_eq!(result.name, "active-support");
        assert_eq!(result.version, "7.1.0");
    }

    #[test]
    fn from_gem_path_rejects_no_gem_extension() {
        assert!(CacheableRequest::from_gem_path("rack-3.0.0").is_none());
    }

    #[test]
    fn from_gem_path_rejects_invalid_format() {
        assert!(CacheableRequest::from_gem_path("invalid.gem").is_none());
    }

    #[test]
    fn from_gem_path_java_platform() {
        let result = CacheableRequest::from_gem_path("jruby-rack-1.1.21-java.gem").unwrap();
        assert_eq!(result.name, "jruby-rack");
        assert_eq!(result.version, "1.1.21");
        assert_eq!(result.platform.as_deref(), Some("java"));
    }

    // ============================================================================
    // CacheableRequest::from_spec_path tests
    // ============================================================================

    #[test]
    fn from_spec_path_simple() {
        let result = CacheableRequest::from_spec_path("rack-3.0.0.gemspec.rz").unwrap();
        assert_eq!(result.kind, AssetKind::Spec);
        assert_eq!(result.name, "rack");
        assert_eq!(result.version, "3.0.0");
        assert!(result.platform.is_none());
        assert_eq!(result.file_name, "rack-3.0.0.gemspec.rz");
        assert_eq!(
            result.relative_path,
            "quick/Marshal.4.8/rack/rack-3.0.0.gemspec.rz"
        );
    }

    #[test]
    fn from_spec_path_with_platform() {
        let result =
            CacheableRequest::from_spec_path("nokogiri-1.15.5-x86_64-linux.gemspec.rz").unwrap();
        assert_eq!(result.kind, AssetKind::Spec);
        assert_eq!(result.name, "nokogiri");
        assert_eq!(result.version, "1.15.5");
        assert_eq!(result.platform.as_deref(), Some("x86_64-linux"));
    }

    #[test]
    fn from_spec_path_rejects_wrong_extension() {
        assert!(CacheableRequest::from_spec_path("rack-3.0.0.gem").is_none());
    }

    #[test]
    fn from_spec_path_rejects_invalid_format() {
        assert!(CacheableRequest::from_spec_path("invalid.gemspec.rz").is_none());
    }

    // ============================================================================
    // CacheableRequest::from_request tests
    // ============================================================================

    #[test]
    fn from_request_gem_path() {
        let result = CacheableRequest::from_request(&req("/gems/rack-3.0.0.gem")).unwrap();
        assert_eq!(result.kind, AssetKind::Gem);
        assert_eq!(result.name, "rack");
        assert_eq!(result.version, "3.0.0");
    }

    #[test]
    fn from_request_spec_path() {
        let result =
            CacheableRequest::from_request(&req("/quick/Marshal.4.8/rack-3.0.0.gemspec.rz"))
                .unwrap();
        assert_eq!(result.kind, AssetKind::Spec);
        assert_eq!(result.name, "rack");
    }

    #[test]
    fn from_request_non_cacheable_path() {
        assert!(CacheableRequest::from_request(&req("/versions")).is_none());
    }

    #[test]
    fn from_request_health_check() {
        assert!(CacheableRequest::from_request(&req("/up")).is_none());
    }

    // ============================================================================
    // CacheableRequest utility methods tests
    // ============================================================================

    #[test]
    fn cacheable_request_download_name() {
        let req = CacheableRequest::from_gem_path("rack-3.0.0.gem").unwrap();
        assert_eq!(req.download_name(), "rack-3.0.0.gem");
    }

    #[test]
    fn cacheable_request_content_type_gem() {
        let req = CacheableRequest::from_gem_path("rack-3.0.0.gem").unwrap();
        assert_eq!(req.content_type(), "application/octet-stream");
    }

    #[test]
    fn cacheable_request_content_type_spec() {
        let req = CacheableRequest::from_spec_path("rack-3.0.0.gemspec.rz").unwrap();
        assert_eq!(req.content_type(), "application/x-deflate");
    }

    #[test]
    fn cacheable_request_asset_key_no_platform() {
        let req = CacheableRequest::from_gem_path("rack-3.0.0.gem").unwrap();
        let key = req.asset_key();
        assert_eq!(key.kind, AssetKind::Gem);
        assert_eq!(key.name, "rack");
        assert_eq!(key.version, "3.0.0");
        assert!(key.platform.is_none());
    }

    #[test]
    fn cacheable_request_asset_key_with_platform() {
        let req = CacheableRequest::from_gem_path("nokogiri-1.15.5-x86_64-darwin.gem").unwrap();
        let key = req.asset_key();
        assert_eq!(key.platform, Some("x86_64-darwin"));
    }

    // ============================================================================
    // UpstreamTarget::from_url tests
    // ============================================================================

    #[test]
    fn upstream_from_url_https_default_port() {
        let url = reqwest::Url::parse("https://rubygems.org").unwrap();
        let upstream = UpstreamTarget::from_url(&url).unwrap();
        assert_eq!(upstream.base.host_str().unwrap(), "rubygems.org");
        assert_eq!(upstream.base.port_or_known_default(), Some(443));
        assert_eq!(upstream.base.scheme(), "https");
    }

    #[test]
    fn upstream_from_url_https_custom_port() {
        let url = reqwest::Url::parse("https://example.com:8443").unwrap();
        let upstream = UpstreamTarget::from_url(&url).unwrap();
        assert_eq!(upstream.base.host_str().unwrap(), "example.com");
        assert_eq!(upstream.base.port_or_known_default(), Some(8443));
    }

    #[test]
    fn upstream_from_url_http_default_port() {
        let url = reqwest::Url::parse("http://localhost").unwrap();
        let upstream = UpstreamTarget::from_url(&url).unwrap();
        assert_eq!(upstream.base.host_str().unwrap(), "localhost");
        assert_eq!(upstream.base.port_or_known_default(), Some(80));
        assert_eq!(upstream.base.scheme(), "http");
    }

    #[test]
    fn upstream_from_url_http_custom_port() {
        let url = reqwest::Url::parse("http://localhost:3000").unwrap();
        let upstream = UpstreamTarget::from_url(&url).unwrap();
        assert_eq!(upstream.base.port_or_known_default(), Some(3000));
        assert_eq!(upstream.base.scheme(), "http");
    }

    #[test]
    fn upstream_from_url_with_path() {
        let url = reqwest::Url::parse("https://rubygems.org/api").unwrap();
        let upstream = UpstreamTarget::from_url(&url).unwrap();
        assert_eq!(upstream.base.as_str(), "https://rubygems.org/api");
    }

    #[test]
    fn upstream_join_simple_path() {
        let url = reqwest::Url::parse("https://rubygems.org").unwrap();
        let upstream = UpstreamTarget::from_url(&url).unwrap();
        let result = upstream.join(&req("/gems/rack-3.0.0.gem")).unwrap();
        assert_eq!(result.as_str(), "https://rubygems.org/gems/rack-3.0.0.gem");
    }

    #[test]
    fn upstream_join_with_base_path() {
        let url = reqwest::Url::parse("https://example.com/api/v1/").unwrap();
        let upstream = UpstreamTarget::from_url(&url).unwrap();
        let result = upstream.join(&req("/gems/rack-3.0.0.gem")).unwrap();
        assert_eq!(
            result.as_str(),
            "https://example.com/api/v1/gems/rack-3.0.0.gem"
        );
    }

    #[test]
    fn upstream_join_root_path() {
        let url = reqwest::Url::parse("https://rubygems.org").unwrap();
        let upstream = UpstreamTarget::from_url(&url).unwrap();
        let result = upstream.join(&req("/")).unwrap();
        assert_eq!(result.as_str(), "https://rubygems.org/");
    }

    #[test]
    fn upstream_join_with_query_string() {
        let url = reqwest::Url::parse("https://rubygems.org").unwrap();
        let upstream = UpstreamTarget::from_url(&url).unwrap();
        let result = upstream
            .join(&req("/api/v1/dependencies?gems=rack"))
            .unwrap();
        assert_eq!(
            result.as_str(),
            "https://rubygems.org/api/v1/dependencies?gems=rack"
        );
    }

    #[test]
    fn upstream_join_no_leading_slash_still_works() {
        let url = reqwest::Url::parse("https://rubygems.org/").unwrap();
        let upstream = UpstreamTarget::from_url(&url).unwrap();
        let result = upstream.join(&req("/gems/rack-3.0.0.gem")).unwrap();
        assert!(result.as_str().contains("/gems/rack-3.0.0.gem"));
    }

    // ============================================================================
    // CacheStatus Display tests
    // ============================================================================

    #[test]
    fn cache_status_display_pass() {
        assert_eq!(CacheStatus::Pass.to_string(), "pass");
    }

    #[test]
    fn cache_status_display_hit() {
        assert_eq!(CacheStatus::Hit.to_string(), "hit");
    }

    #[test]
    fn cache_status_display_miss() {
        assert_eq!(CacheStatus::Miss.to_string(), "miss");
    }

    #[test]
    fn cache_status_display_revalidated() {
        assert_eq!(CacheStatus::Revalidated.to_string(), "revalidated");
    }

    #[test]
    fn cache_status_display_error() {
        assert_eq!(CacheStatus::Error.to_string(), "error");
    }

    #[test]
    fn cache_status_equality() {
        assert_eq!(CacheStatus::Hit, CacheStatus::Hit);
        assert_ne!(CacheStatus::Hit, CacheStatus::Miss);
    }

    // ============================================================================
    // RequestContext tests
    // ============================================================================

    #[test]
    fn request_context_default() {
        let ctx = RequestContext::default();
        assert_eq!(ctx.method, Method::GET);
        assert_eq!(ctx.path, "");
        assert_eq!(ctx.cache, CacheStatus::Pass);
        assert!(ctx.start.elapsed().as_millis() < 100);
    }

    // ============================================================================
    // Edge cases and error paths
    // ============================================================================

    #[test]
    fn from_gem_path_empty_string() {
        assert!(CacheableRequest::from_gem_path("").is_none());
    }

    #[test]
    fn from_spec_path_empty_string() {
        assert!(CacheableRequest::from_spec_path("").is_none());
    }

    #[test]
    fn from_gem_path_just_extension() {
        assert!(CacheableRequest::from_gem_path(".gem").is_none());
    }

    #[test]
    fn from_spec_path_just_extension() {
        assert!(CacheableRequest::from_spec_path(".gemspec.rz").is_none());
    }

    #[test]
    fn upstream_join_handles_special_chars() {
        let url = reqwest::Url::parse("https://rubygems.org").unwrap();
        let upstream = UpstreamTarget::from_url(&url).unwrap();
        let result = upstream.join(&req("/info/my%2Dgem")).unwrap();
        assert!(result.as_str().contains("my%2Dgem"));
    }

    #[test]
    fn cacheable_request_relative_path_structure_gem() {
        let req = CacheableRequest::from_gem_path("rails-7.1.0.gem").unwrap();
        assert_eq!(req.relative_path, "gems/rails/rails-7.1.0.gem");
    }

    #[test]
    fn cacheable_request_relative_path_structure_spec() {
        let req = CacheableRequest::from_spec_path("rails-7.1.0.gemspec.rz").unwrap();
        assert_eq!(
            req.relative_path,
            "quick/Marshal.4.8/rails/rails-7.1.0.gemspec.rz"
        );
    }
}
