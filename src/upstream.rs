use anyhow::{Result, anyhow};
use breaker_machines::CircuitBreaker;
use chrono_machines::{BackoffPolicy, BackoffStrategy, ConstantBackoff, ExponentialBackoff, FibonacciBackoff};
use parking_lot::Mutex;
use rama::{
    Service,
    http::{
        Body, Method, Request, Response, Uri,
        body::util::BodyExt as _,
        client::EasyHttpWebClient,
        header::{HeaderMap, HeaderValue, USER_AGENT},
        layer::trace::TraceLayer,
    },
    layer::Layer,
    telemetry::tracing,
};
use rand::SeedableRng;
use rand::rngs::SmallRng;
use std::sync::Arc;
use tracing::{info, warn};

use crate::config::{BackoffStrategy as ConfigBackoffStrategy, UpstreamConfig};

const UA: &str = concat!("vein/", env!("CARGO_PKG_VERSION"));

/// Rama-based upstream HTTP client with retry, circuit breaker, and tracing.
#[derive(Clone)]
pub struct UpstreamClient {
    pub backoff: BackoffPolicy,
    pub breaker: Arc<Mutex<CircuitBreaker>>,
}

impl UpstreamClient {
    pub fn new(config: &UpstreamConfig) -> Result<Self> {
        // Configure circuit breaker for upstream resilience
        // Open circuit after 5 server errors in 60s window, reset after 30s
        let breaker = CircuitBreaker::builder("rubygems_upstream")
            .failure_threshold(5)
            .failure_window_secs(60.0)
            .half_open_timeout_secs(30.0)
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
            .build();

        let retry = &config.reliability.retry;
        let max_delay_ms = retry.max_backoff_secs * 1000;
        let jitter = retry.jitter_factor;
        let max_attempts = retry.max_attempts as u8;

        let backoff: BackoffPolicy = match retry.backoff_strategy {
            ConfigBackoffStrategy::Exponential => ExponentialBackoff::new()
                .base_delay_ms(retry.initial_backoff_ms)
                .max_delay_ms(max_delay_ms)
                .max_attempts(max_attempts)
                .jitter_factor(jitter)
                .into(),
            ConfigBackoffStrategy::Fibonacci => FibonacciBackoff::new()
                .base_delay_ms(retry.initial_backoff_ms)
                .max_delay_ms(max_delay_ms)
                .max_attempts(max_attempts)
                .jitter_factor(jitter)
                .into(),
            ConfigBackoffStrategy::Constant => ConstantBackoff::new()
                .delay_ms(retry.initial_backoff_ms)
                .max_attempts(max_attempts)
                .jitter_factor(jitter)
                .into(),
        };

        info!(
            timeout_secs = config.timeout_secs,
            pool = config.connection_pool_size,
            strategy = ?retry.backoff_strategy,
            max_attempts = max_attempts,
            "Upstream client initialized (rama + rustls + circuit breaker + chrono-machines)",
        );

        Ok(Self {
            backoff,
            breaker: Arc::new(Mutex::new(breaker)),
        })
    }

    pub async fn get_with_headers(&self, url: Uri, headers: &HeaderMap) -> Result<Response<Body>> {
        // Check if circuit is open before attempting request
        if self.breaker.lock().is_open() {
            return Err(anyhow!(
                "Circuit breaker is open - upstream is currently unavailable"
            ));
        }

        let client = (TraceLayer::new_for_http(),).into_layer(EasyHttpWebClient::default());
        let mut attempt: u8 = 0;
        let mut rng = SmallRng::from_os_rng();
        let start_time = std::time::Instant::now();
        let max_attempts = self.backoff.max_attempts();

        loop {
            attempt += 1;

            let mut builder = Request::builder().method(Method::GET).uri(url.clone());
            {
                let h = builder
                    .headers_mut()
                    .ok_or_else(|| anyhow!("cannot get headers mut"))?;
                for (name, value) in headers {
                    h.insert(name, value.clone());
                }
                h.insert(USER_AGENT, HeaderValue::from_static(UA));
            }

            let request = builder
                .body(Body::empty())
                .map_err(|e| anyhow!("building upstream request: {e}"))?;

            match client.serve(request).await {
                Ok(response) if response.status().is_server_error() && attempt < max_attempts => {
                    // Server error but we have retries left - use chrono-machines backoff
                    if let Some(delay_ms) = self.backoff.delay(attempt, &mut rng) {
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                    continue;
                }
                Ok(response) => {
                    let status = response.status();
                    let duration = start_time.elapsed().as_secs_f64();

                    // Record success or failure based on status code
                    if status.is_server_error() {
                        // This is the final attempt and it failed with 5xx
                        let mut breaker = self.breaker.lock();
                        breaker.record_failure(duration);
                        breaker.check_and_trip();
                    } else {
                        // Success or client error (4xx) - don't trip on client errors
                        self.breaker.lock().record_success(duration);
                    }

                    let resp_headers = response.headers().clone();
                    let body_bytes = response.into_body().collect().await?.to_bytes();

                    let mut builder = Response::builder().status(status);
                    {
                        let h = builder
                            .headers_mut()
                            .ok_or_else(|| anyhow!("cannot get headers mut"))?;
                        for (name, value) in resp_headers.iter() {
                            h.insert(name, value.clone());
                        }
                    }

                    return builder
                        .body(Body::from(body_bytes))
                        .map_err(|e| anyhow!("rebuilding upstream response: {e}"));
                }
                Err(err) => {
                    let duration = start_time.elapsed().as_secs_f64();

                    if attempt >= max_attempts {
                        // All retries exhausted - record failure and try to trip
                        let mut breaker = self.breaker.lock();
                        breaker.record_failure(duration);
                        breaker.check_and_trip();
                        return Err(anyhow!("upstream request failed: {err}"));
                    }

                    // Use chrono-machines backoff with jitter
                    if let Some(delay_ms) = self.backoff.delay(attempt, &mut rng) {
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                }
            }
        }
    }
}
