use anyhow::Result;
use rama::{
    Service,
    error::BoxError,
    http::{Body, Method, Request, Response, StatusCode},
    telemetry::tracing::{error, info},
};

#[cfg(any(feature = "crates", feature = "npm"))]
use crate::http_cache::CacheOutcome;

#[cfg(feature = "gems")]
use super::compact::CompactRequest;
use super::{
    CacheStatus, RequestContext, VeinProxy, handlers, public_base::public_base, response, route,
};

#[cfg(feature = "crates")]
use crate::crates as crates_registry;
#[cfg(feature = "npm")]
use crate::npm as npm_registry;
use crate::registry::{self, NotBuilt};
use crate::search;

impl VeinProxy {
    async fn handle(&self, req: Request<Body>, ctx: &mut RequestContext) -> Result<Response<Body>> {
        let method = req.method().clone();
        let path = req.uri().path_or_root().into_owned();
        // What every URL handed back must use: the bind is often loopback.
        let our_base = public_base(req.headers(), &self.config.server);

        let registry = route::registry_of(&req);
        if let Some(kind) = registry.filter(|&kind| !registry::built(kind)) {
            ctx.cache = CacheStatus::Pass;
            return response::respond_text(StatusCode::NOT_FOUND, &format!("{}\n", NotBuilt(kind)));
        }

        if method == Method::GET
            && let Some(query) = search::Query::parse(req.uri())
        {
            ctx.cache = CacheStatus::Pass;
            let target = req.uri().request_target();
            return search::respond(&self.http, &self.index, &self.search, &query, &target).await;
        }

        if method == Method::GET && path.starts_with("/docs/") {
            let docs = crate::docs::Docs::new(
                self.config.clone(),
                self.index.clone(),
                self.storage.clone(),
            );
            if let Some(answer) = crate::docs::http::respond(&docs, &path).await {
                ctx.cache = CacheStatus::Pass;
                return answer;
            }
        }

        #[cfg(feature = "npm")]
        if registry == Some(vein_adapter::AssetKind::NpmPackage) {
            let result = npm_registry::handle_npm_request(
                &self.http,
                req,
                &our_base,
                self.storage.clone(),
                self.index.clone(),
                &self.config.delay_policy,
            )
            .await;
            return finish_registry_result(ctx, result, "npm request failed", "npm upstream error");
        }

        if method == Method::GET {
            match path.as_str() {
                "/up" => {
                    let (resp, status) = handlers::handle_health(self.index.as_ref()).await?;
                    ctx.cache = status;
                    return Ok(resp);
                }
                #[cfg(feature = "gems")]
                "/.well-known/vein/sbom" => {
                    let (resp, status) =
                        super::sbom::handle_sbom_request(&req, self.index.as_ref()).await?;
                    ctx.cache = status;
                    return Ok(resp);
                }
                "/" => {
                    ctx.cache = CacheStatus::Pass;
                    return response::respond_homepage(&self.config, &our_base);
                }
                #[cfg(feature = "crates")]
                p if p.starts_with("/index/") => {
                    let result = crates_registry::handle_sparse_index(
                        &self.http,
                        p,
                        &our_base,
                        self.storage.clone(),
                        self.index.clone(),
                        &self.config.delay_policy,
                    )
                    .await;
                    return finish_registry_result(
                        ctx,
                        result,
                        "crates index request failed",
                        "upstream error",
                    );
                }
                _ => {}
            }
        }

        if let Some((cache_status, resp)) = self.try_handle_cached_request(&req, ctx).await? {
            ctx.cache = cache_status;
            Ok(resp)
        } else {
            if self.upstreams.is_empty() {
                ctx.cache = CacheStatus::Miss;
                return response::respond_text(StatusCode::NOT_FOUND, "not found in cache");
            }

            if method == Method::GET {
                #[cfg(feature = "gems")]
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
                            summary = %Self::request_summary(ctx),
                            "failed to proxy request to upstream"
                        );
                        return response::respond_text(StatusCode::BAD_GATEWAY, "upstream error");
                    }
                }
            }

            ctx.cache = CacheStatus::Pass;
            response::respond_text(StatusCode::BAD_REQUEST, "unsupported request")
        }
    }

    fn request_summary(ctx: &RequestContext) -> String {
        format!("{} {}", ctx.method.as_str(), ctx.path)
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
                    summary = %Self::request_summary(&ctx),
                    response_code,
                    duration_ms,
                    cache_status = %ctx.cache,
                    "request handled"
                );
            }
            Err(err) => {
                let duration_ms = ctx.start.elapsed().as_millis();
                error!(
                    summary = %Self::request_summary(&ctx),
                    duration_ms,
                    cache_status = %ctx.cache,
                    error = %err,
                    "request failed"
                );
            }
        }

        result.map_err(std::convert::Into::into)
    }
}

/// Finalizes a registry handler result: records cache status on success, or
/// logs the error and returns a `502 Bad Gateway` body on failure.
#[cfg(any(feature = "crates", feature = "npm"))]
fn finish_registry_result(
    ctx: &mut RequestContext,
    result: Result<(Response<Body>, CacheOutcome)>,
    error_log: &str,
    error_body: &'static str,
) -> Result<Response<Body>> {
    match result {
        Ok((resp, outcome)) => {
            ctx.cache = cache_status_from_outcome(outcome);
            Ok(resp)
        }
        Err(err) => {
            error!(error = %err, message = error_log);
            ctx.cache = CacheStatus::Error;
            response::respond_text(StatusCode::BAD_GATEWAY, error_body)
        }
    }
}

#[cfg(any(feature = "crates", feature = "npm"))]
const fn cache_status_from_outcome(outcome: CacheOutcome) -> CacheStatus {
    match outcome {
        CacheOutcome::Hit => CacheStatus::Hit,
        CacheOutcome::Miss => CacheStatus::Miss,
        CacheOutcome::Revalidated => CacheStatus::Revalidated,
        CacheOutcome::Pass => CacheStatus::Pass,
    }
}
