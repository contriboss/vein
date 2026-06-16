use anyhow::Result;
use rama::{
    Service,
    error::BoxError,
    http::{Body, Method, Request, Response, StatusCode},
    telemetry::tracing::{error, info},
};

use crate::http_cache::CacheOutcome;

use super::{CacheStatus, RequestContext, VeinProxy, compact::CompactRequest, handlers, response};

use crate::{crates as crates_registry, npm as npm_registry};

impl VeinProxy {
    async fn handle(&self, req: Request<Body>, ctx: &mut RequestContext) -> Result<Response<Body>> {
        let method = req.method().clone();
        let path = req.uri().path().to_owned();

        if npm_registry::is_npm_request(&req) {
            let our_base = format!(
                "http://{}:{}",
                self.config.server.host, self.config.server.port
            );
            let result = npm_registry::handle_npm_request(
                req,
                &our_base,
                self.storage.clone(),
                self.index.clone(),
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
                p if p.starts_with("/index/") => {
                    let our_base = format!(
                        "http://{}:{}",
                        self.config.server.host, self.config.server.port
                    );
                    let result = crates_registry::handle_sparse_index(
                        p,
                        &our_base,
                        self.storage.clone(),
                        self.index.clone(),
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

        match self.try_handle_cached_request(&req, ctx).await? {
            Some((cache_status, resp)) => {
                ctx.cache = cache_status;
                Ok(resp)
            }
            None => {
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
                response::respond_text(StatusCode::BAD_REQUEST, "unsupported request")
            }
        }
    }

    fn request_summary(&self, ctx: &RequestContext) -> String {
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

/// Finalizes a registry handler result: records cache status on success, or
/// logs the error and returns a `502 Bad Gateway` body on failure.
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

fn cache_status_from_outcome(outcome: CacheOutcome) -> CacheStatus {
    match outcome {
        CacheOutcome::Hit => CacheStatus::Hit,
        CacheOutcome::Miss => CacheStatus::Miss,
        CacheOutcome::Revalidated => CacheStatus::Revalidated,
        CacheOutcome::Pass => CacheStatus::Pass,
    }
}
