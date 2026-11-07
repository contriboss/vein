use anyhow::{Context, Result, anyhow};
#[cfg(feature = "crates")]
use rama::Service;
use rama::http::{Body, HeaderMap, HeaderValue, Method, Request, Response, header};
use vein_adapter::CacheBackendTrait;

#[cfg(feature = "crates")]
use crate::upstream::UA;
use crate::upstream::buffer_response;

use super::{CacheStatus, RequestContext, VeinProxy, cache, types};
use crate::delay;

impl VeinProxy {
    pub(super) async fn try_handle_cached_request(
        &self,
        req: &Request<Body>,
        ctx: &mut RequestContext,
    ) -> Result<Option<(CacheStatus, Response<Body>)>> {
        use types::CacheableRequest;

        ctx.method = req.method().clone();
        ctx.path = req.uri().path_or_root().into_owned();

        if req.method() != Method::GET {
            return Ok(None);
        }

        let Some(cacheable) = CacheableRequest::from_request(req) else {
            return Ok(None);
        };

        if delay::is_blocked(
            &self.config.delay_policy,
            &self.index,
            &cacheable.asset_key(),
        )
        .await
        {
            let response = Response::builder()
                .status(rama::http::StatusCode::FORBIDDEN)
                .header(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"))
                .body(Body::from(delay::BLOCKED))
                .context("building blocked response")?;
            return Ok(Some((CacheStatus::Pass, response)));
        }

        match self.index.get(&cacheable.asset_key()).await? {
            Some(entry) => match cache::serve_cached(&cacheable, entry, &self.storage).await {
                Ok(resp) => Ok(Some((CacheStatus::Hit, resp))),
                Err(err) => {
                    rama::telemetry::tracing::warn!(
                        error = %err,
                        "failed to serve cached asset, falling back to upstream"
                    );
                    self.fetch_and_stream(req, &cacheable, true)
                        .await
                        .map(|resp| Some((CacheStatus::Revalidated, resp)))
                }
            },
            None => self
                .fetch_and_stream(req, &cacheable, false)
                .await
                .map(|resp| Some((CacheStatus::Miss, resp))),
        }
    }

    async fn fetch_and_stream(
        &self,
        req: &Request<Body>,
        cacheable: &types::CacheableRequest,
        treating_as_revalidation: bool,
    ) -> Result<Response<Body>> {
        let response = match cacheable.kind {
            #[cfg(feature = "crates")]
            vein_adapter::AssetKind::Crate => {
                self.fetch_crate(&cacheable.name, &cacheable.version)
                    .await?
            }
            _ => self
                .fetch_with_fallback(req, None)
                .await
                .context("requesting upstream")?,
        };

        if !response.status().is_success() {
            rama::telemetry::tracing::warn!(
                status = %response.status(),
                path = %req.uri().path_or_root(),
                "upstream returned error status"
            );
            return self.forward_response(response).await;
        }

        let temp_file = self
            .storage
            .create_temp_writer(&cacheable.relative_path)
            .await
            .context("creating temp file")?;

        cache::run_cache_miss_flow(
            cacheable,
            self.index.clone(),
            self.storage.clone(),
            response,
            temp_file,
            treating_as_revalidation,
        )
        .await
    }

    /// Fetches a crate from crates.io's CDN, which serves the downloads
    /// (the index names it in `dl`); the cache flow stores it like any asset.
    #[cfg(feature = "crates")]
    async fn fetch_crate(&self, name: &str, version: &str) -> Result<Response<Body>> {
        let url = format!("https://static.crates.io/crates/{name}/{name}-{version}.crate");
        let request = Request::builder()
            .method(Method::GET)
            .uri(url.as_str())
            .header(header::USER_AGENT, HeaderValue::from_static(UA))
            .body(Body::empty())
            .context("building crate request")?;
        self.http
            .serve(request)
            .await
            .map_err(|e| anyhow!("crate fetch failed: {e}"))
    }

    async fn forward_response(&self, response: Response<Body>) -> Result<Response<Body>> {
        buffer_response(response, true)
            .await
            .context("buffering forwarded response")
    }

    pub(super) async fn proxy_generic_get(&self, req: &Request<Body>) -> Result<Response<Body>> {
        let response = self
            .fetch_with_fallback(req, None)
            .await
            .context("requesting upstream")?;
        self.forward_response(response).await
    }

    pub(super) async fn fetch_with_fallback(
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
        // Whether every upstream refused at its breaker, and when each
        // may be tried again: then the client is told when to come back.
        let mut unavailable = Vec::new();
        let mut all_unavailable = true;
        let no_headers = HeaderMap::new();

        for upstream in &self.upstreams {
            let upstream_url = upstream
                .join(req)
                .with_context(|| format!("constructing upstream url for {}", upstream.base))?;

            let response = client
                .get_with_headers(upstream_url.clone(), headers.unwrap_or(&no_headers))
                .await;

            match response {
                Ok(r) if r.status().is_server_error() => {
                    all_unavailable = false;
                    last_err = Some(anyhow!("{} returned {}", upstream_url, r.status()));
                }
                Ok(r) => return Ok(r),
                Err(err) => match err.downcast::<crate::upstream::Unavailable>() {
                    Ok(refused) => unavailable.push(refused),
                    Err(err) => {
                        all_unavailable = false;
                        last_err = Some(anyhow!("{upstream_url} failed: {err}"));
                    }
                },
            }
        }

        if all_unavailable && let Some(refused) = crate::upstream::Unavailable::every(unavailable) {
            return Err(refused.into());
        }
        Err(last_err.unwrap_or_else(|| anyhow!("all upstreams failed")))
    }
}
