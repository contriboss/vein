use anyhow::{Context, Result, anyhow};
use rama::{
    Service,
    http::{Body, HeaderMap, HeaderValue, Method, Request, Response, body::util::BodyExt, header},
};
use vein_adapter::CacheBackendTrait;

use super::{CacheStatus, RequestContext, VeinProxy, cache, quarantine, types};

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
        let response = if cacheable.kind == vein_adapter::AssetKind::Crate {
            self.fetch_crate(&cacheable.name, &cacheable.version)
                .await?
        } else {
            self.fetch_with_fallback(req, None)
                .await
                .context("requesting upstream")?
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

        let result = cache::run_cache_miss_flow(
            cacheable,
            self.index.clone(),
            self.storage.clone(),
            response,
            temp_file,
            treating_as_revalidation,
        )
        .await;

        if result.is_ok()
            && cacheable.kind == vein_adapter::AssetKind::Gem
            && let Err(err) = quarantine::record_new_version(
                &self.config.delay_policy,
                self.index.as_ref(),
                &cacheable.name,
                &cacheable.version,
                cacheable.platform.as_deref(),
                "",
            )
            .await
        {
            rama::telemetry::tracing::warn!(
                error = %err,
                gem = %cacheable.name,
                version = %cacheable.version,
                "Failed to record version in quarantine system"
            );
        }

        result
    }

    /// Fetch a crate from crates.io CDN.
    async fn fetch_crate(&self, name: &str, version: &str) -> Result<Response<Body>> {
        use rama::http::client::EasyHttpWebClient;

        let url = format!(
            "https://static.crates.io/crates/{}/{}-{}.crate",
            name, name, version
        );

        let client = EasyHttpWebClient::default();

        let request = Request::builder()
            .method(Method::GET)
            .uri(url.as_str())
            .header(
                header::USER_AGENT,
                HeaderValue::from_static(concat!("vein/", env!("CARGO_PKG_VERSION"))),
            )
            .body(Body::empty())
            .context("building crate request")?;

        let response = client
            .serve(request)
            .await
            .map_err(|e| anyhow!("crate fetch failed: {e}"))?;

        let status = response.status();
        let headers = response.headers().clone();
        let body_bytes = response
            .into_body()
            .collect()
            .await
            .context("reading crate response body")?
            .to_bytes();

        let mut builder = Response::builder().status(status);
        {
            let resp_headers = builder
                .headers_mut()
                .ok_or_else(|| anyhow!("cannot get headers mut"))?;
            for (name, value) in headers.iter() {
                resp_headers.insert(name, value.clone());
            }
        }

        builder
            .body(Body::from(body_bytes))
            .context("building crate response")
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

        for upstream in &self.upstreams {
            let upstream_url = upstream
                .join(req)
                .with_context(|| format!("constructing upstream url for {}", upstream.base))?;

            let response = client
                .get_with_headers(upstream_url.clone(), headers.unwrap_or(&HeaderMap::new()))
                .await;

            match response {
                Ok(r) if r.status().is_server_error() => {
                    last_err = Some(anyhow!("{} returned {}", upstream_url, r.status()));
                }
                Ok(r) => return Ok(r),
                Err(err) => {
                    last_err = Some(anyhow!("{} failed: {err}", upstream_url));
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow!("all upstreams failed")))
    }
}
