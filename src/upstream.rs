use anyhow::{Context as _, Result, anyhow};
use breaker_machines::{AsyncCircuitBreaker, CircuitError};
use chrono_machines::{AsyncRetryable, BackoffPolicy, DelayHint};
use rama::{
    Service,
    error::BoxError,
    http::{
        Body, Method, Request, Response, StatusCode, StreamingBody,
        body::util::BodyExt as _,
        client::DefaultHttpWebClient,
        header::{self, HeaderMap, HeaderValue, USER_AGENT},
        headers::{After, HeaderMapExt as _, RetryAfter},
        layer::trace::TraceLayer,
    },
    layer::Layer,
    net::uri::Uri,
    telemetry::tracing::{info, warn},
};
use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

use crate::config::UpstreamConfig;

pub const UA: &str = concat!("vein/", env!("CARGO_PKG_VERSION"));

/// Performs a one-shot upstream `GET` without retry or circuit breaker.
///
/// Sets the vein user-agent and forwards `extra_headers`. When `accept` is
/// provided it is applied only if `extra_headers` did not already set an
/// `Accept` header.
pub async fn simple_get(
    client: &DefaultHttpWebClient,
    url: &str,
    extra_headers: &HeaderMap,
    accept: Option<&str>,
) -> Result<Response<Body>> {
    let mut builder = Request::builder().method(Method::GET).uri(url);
    {
        let headers = builder
            .headers_mut()
            .ok_or_else(|| anyhow!("cannot get headers mut"))?;
        headers.extend(extra_headers.clone());
        headers.insert(USER_AGENT, HeaderValue::from_static(UA));
        if let Some(accept) = accept
            && !headers.contains_key(header::ACCEPT)
        {
            headers.insert(header::ACCEPT, HeaderValue::from_str(accept)?);
        }
    }
    let request = builder
        .body(Body::empty())
        .map_err(|e| anyhow!("building upstream request: {e}"))?;
    client
        .serve(request)
        .await
        .map_err(|e| anyhow!("upstream request failed: {e}"))
}

/// Reads `response`'s body into memory and rebuilds it with the same status
/// and headers, repeated headers included.
///
/// `Transfer-Encoding` is dropped when `strip_transfer_encoding` is set, since
/// the body is no longer chunked.
pub async fn buffer_response<B>(
    response: Response<B>,
    strip_transfer_encoding: bool,
) -> Result<Response<Body>>
where
    B: StreamingBody<Error: Into<BoxError>>,
{
    let (parts, body) = response.into_parts();
    let bytes = body
        .collect()
        .await
        .map_err(|e| anyhow::Error::from_boxed(e.into()))
        .context("reading upstream response body")?
        .to_bytes();

    let mut buffered = Response::new(Body::from(bytes));
    *buffered.status_mut() = parts.status;
    *buffered.headers_mut() = parts.headers;
    if strip_transfer_encoding {
        buffered.headers_mut().remove(header::TRANSFER_ENCODING);
    }
    Ok(buffered)
}

/// Why a single upstream attempt is worth retrying.
enum AttemptError {
    /// Upstream answered, but with a 5xx or 429. Kept so the last answer can
    /// be handed to the caller once retries run out.
    Status(Box<Response<Body>>),
    /// Connection, TLS, or body-read failure.
    Transport(anyhow::Error),
}

impl fmt::Display for AttemptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Status(response) => write!(f, "upstream returned {}", response.status()),
            Self::Transport(err) => write!(f, "{err:#}"),
        }
    }
}

impl AttemptError {
    /// The upstream's `Retry-After`, if it sent a usable one.
    fn retry_after(&self) -> Option<Duration> {
        let Self::Status(response) = self else {
            return None;
        };
        match response.headers().typed_get::<RetryAfter>()?.after() {
            After::Delay(seconds) => Some(seconds.into()),
            After::DateTime(date) => SystemTime::from(date)
                .duration_since(SystemTime::now())
                .ok(),
        }
    }
}

/// Rama-based upstream HTTP client with retry, per-upstream circuit breakers,
/// tracing, and a pooled connection set shared by every clone.
#[derive(Clone)]
pub struct UpstreamClient {
    backoff: BackoffPolicy,
    http: DefaultHttpWebClient,
    breakers: Arc<Mutex<HashMap<String, Arc<AsyncCircuitBreaker>>>>,
    half_open_timeout_secs: f64,
}

impl UpstreamClient {
    pub fn new(config: &UpstreamConfig) -> Result<Self> {
        Ok(Self {
            backoff: config.reliability.retry.backoff_policy(),
            http: DefaultHttpWebClient::default(),
            breakers: Arc::default(),
            half_open_timeout_secs: 30.0,
        })
    }

    /// The breaker guarding `url`'s host. Each upstream trips on its own, so
    /// a failing primary does not also lock out its fallbacks.
    fn breaker_for(&self, url: &Uri) -> Arc<AsyncCircuitBreaker> {
        let key = url
            .authority()
            .map(|authority| authority.to_string())
            .unwrap_or_default();
        let mut breakers = self
            .breakers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        breakers
            .entry(key)
            .or_insert_with_key(|key| Arc::new(build_breaker(key, self.half_open_timeout_secs)))
            .clone()
    }

    /// `GET url`, retrying 5xx, 429, and transport failures with backoff (or
    /// the upstream's `Retry-After`). The whole retry sequence counts as one
    /// call against the upstream's circuit breaker; 429s never trip it.
    ///
    /// Once retries run out on a 5xx or 429, that response is returned as
    /// `Ok` so callers can relay it.
    pub async fn get_with_headers(&self, url: Uri, headers: &HeaderMap) -> Result<Response<Body>> {
        let breaker = self.breaker_for(&url);
        match breaker.call(|| self.fetch_with_retry(&url, headers)).await {
            Ok(response) => Ok(response),
            Err(CircuitError::Execution(AttemptError::Status(response))) => Ok(*response),
            Err(CircuitError::Execution(AttemptError::Transport(err))) => Err(err),
            Err(err) => Err(anyhow!("upstream {url} unavailable: {err}")),
        }
    }

    async fn fetch_with_retry(
        &self,
        url: &Uri,
        headers: &HeaderMap,
    ) -> Result<Response<Body>, AttemptError> {
        let result = (|| self.attempt(url, headers))
            .retry_async(self.backoff)
            .delay_from(|err: &AttemptError, _attempt| match err.retry_after() {
                Some(delay) => DelayHint::Ms(u64::try_from(delay.as_millis()).unwrap_or(u64::MAX)),
                None => DelayHint::Backoff,
            })
            .call_async(crate::util::sleep_ms)
            .await;

        match result {
            Ok(outcome) => Ok(outcome.into_inner()),
            Err(err) => match err.into_cause() {
                // Being rate limited says nothing about upstream health.
                Some(AttemptError::Status(response))
                    if response.status() == StatusCode::TOO_MANY_REQUESTS =>
                {
                    Ok(*response)
                }
                Some(cause) => Err(cause),
                None => Err(AttemptError::Transport(anyhow!(
                    "upstream retry for {url} ended without a cause"
                ))),
            },
        }
    }

    async fn attempt(
        &self,
        url: &Uri,
        headers: &HeaderMap,
    ) -> Result<Response<Body>, AttemptError> {
        let mut request = Request::new(Body::empty());
        *request.method_mut() = Method::GET;
        *request.uri_mut() = url.clone();
        *request.headers_mut() = headers.clone();
        request
            .headers_mut()
            .insert(USER_AGENT, HeaderValue::from_static(UA));

        let client = TraceLayer::new_for_http().into_layer(self.http.clone());
        let response = client
            .serve(request)
            .await
            .map_err(|e| AttemptError::Transport(anyhow!("upstream request failed: {e}")))?;
        // Buffer inside the attempt so a body cut short is retried like any
        // other transport failure.
        let response = buffer_response(response, false)
            .await
            .map_err(AttemptError::Transport)?;

        let status = response.status();
        if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS {
            Err(AttemptError::Status(Box::new(response)))
        } else {
            Ok(response)
        }
    }
}

fn build_breaker(name: &str, half_open_timeout_secs: f64) -> AsyncCircuitBreaker {
    // Open after 5 failed calls in 60s, then probe again once the timeout lapses.
    AsyncCircuitBreaker::builder(format!("upstream:{name}"))
        .failure_threshold(5)
        .failure_window_secs(60.0)
        .half_open_timeout_secs(half_open_timeout_secs)
        .success_threshold(2)
        .jitter_factor(0.1) // 10% jitter on reset timeout
        .on_open(|name| {
            warn!(circuit = %name, "Circuit breaker opened - upstream is failing");
        })
        .on_close(|name| {
            info!(circuit = %name, "Circuit breaker closed - upstream recovered");
        })
        .on_half_open(|name| {
            info!(circuit = %name, "Circuit breaker half-open - testing upstream");
        })
        .build_async()
}

#[cfg(test)]
mod tests;
