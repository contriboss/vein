use std::sync::Arc;

#[cfg(test)]
use anyhow::anyhow;
use anyhow::{Context, Result};
use chrono_machines::{AsyncRetryable, retry::RetryErrorKind};
use rama::telemetry::tracing::{self, debug, error, info, warn};

use crate::config::{Config, DatabaseBackend};
use vein_adapter::CacheBackend;

pub async fn connect_cache_backend(
    config: &Config,
) -> Result<(Arc<CacheBackend>, DatabaseBackend)> {
    let backend = config.database.backend()?;
    let retry_config = &config.database.reliability.retry;

    if retry_config.enabled {
        info!(
            max_attempts = retry_config.max_attempts,
            initial_backoff_ms = retry_config.initial_backoff_ms,
            strategy = ?retry_config.backoff_strategy,
            "Retry enabled for database connection"
        );
    }

    let max_connections = config.database.max_connections;
    let cache = connect_with_retry(
        || async { CacheBackend::connect(&backend.url, max_connections).await },
        retry_config,
        "postgres",
    )
    .await
    .context("connecting postgres cache")?;
    Ok((Arc::new(cache), backend))
}

/// Execute a database connection with retry logic
async fn connect_with_retry<F, Fut, T>(
    connect_fn: F,
    retry_config: &crate::config::RetryConfig,
    db_type: &'static str,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    if !retry_config.enabled {
        debug!(db_type, "Retry disabled, attempting single connection");
        let mut connect_fn = connect_fn;
        return connect_fn().await;
    }

    let max_attempts = retry_config.attempts();
    let result = connect_fn
        .retry_async(retry_config.backoff_policy())
        .when(is_retryable_error)
        .notify(move |ctx| {
            warn!(
                attempt = ctx.attempt,
                max_attempts,
                backoff_ms = ctx.next_delay_ms,
                db_type,
                error = ctx.error.map(tracing::field::display),
                "Database connection failed, retrying"
            );
        })
        .call_async(crate::util::sleep_ms)
        .await;

    match result {
        Ok(outcome) => {
            if outcome.attempts() > 1 {
                info!(
                    attempts = outcome.attempts(),
                    db_type, "Database connection succeeded after retry"
                );
            }
            Ok(outcome.into_inner())
        }
        Err(err) => {
            let attempts = err.attempts();
            let non_retryable = err.kind() == RetryErrorKind::PredicateRejected;
            let err = err.into_cause();
            if non_retryable {
                error!(
                    attempts,
                    db_type,
                    error = %err,
                    "Database connection failed with non-retryable error"
                );
            } else {
                error!(
                    attempts,
                    db_type,
                    error = %err,
                    "Database connection failed after max retries"
                );
            }
            Err(err)
        }
    }
}

/// Determine if a database error is retryable
fn is_retryable_error(err: &anyhow::Error) -> bool {
    let err_str = err.to_string().to_lowercase();

    // Non-retryable errors (authentication, invalid config, etc.)
    if err_str.contains("authentication")
        || err_str.contains("permission denied")
        || err_str.contains("invalid")
        || err_str.contains("malformed")
        || err_str.contains("syntax error")
        || err_str.contains("no such table")
        || err_str.contains("does not exist")
    {
        debug!(error = %err, "Non-retryable database error detected");
        return false;
    }

    // Retryable errors (connection issues, transient failures)
    if err_str.contains("connection")
        || err_str.contains("timeout")
        || err_str.contains("refused")
        || err_str.contains("too many")
        || err_str.contains("busy")
        || err_str.contains("locked")
        || err_str.contains("unavailable")
        || err_str.contains("network")
    {
        debug!(error = %err, "Retryable database error detected");
        return true;
    }

    // Default: assume retryable for unknown errors
    debug!(error = %err, "Unknown error type, treating as retryable");
    true
}

#[cfg(test)]
mod tests;
