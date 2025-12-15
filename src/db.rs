use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use tracing::{debug, error, info, warn};

use crate::config::{BackoffStrategy, Config, DatabaseBackend};
use vein_adapter::{CacheBackend, PostgresCacheBackend, SqliteCacheBackend};

pub async fn connect_cache_backend(
    config: &Config,
) -> Result<(Arc<dyn CacheBackend>, DatabaseBackend)> {
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

    let cache: Arc<dyn CacheBackend> = match &backend {
        DatabaseBackend::Sqlite { path } => {
            let backend = connect_with_retry(
                || async { SqliteCacheBackend::connect(path).await },
                retry_config,
                "sqlite",
            )
            .await
            .context("connecting sqlite cache")?;
            Arc::new(backend)
        }
        DatabaseBackend::Postgres {
            url,
            max_connections,
        } => {
            let backend = connect_with_retry(
                || async { PostgresCacheBackend::connect(url, *max_connections).await },
                retry_config,
                "postgres",
            )
            .await
            .context("connecting postgres cache")?;
            Arc::new(backend)
        }
    };
    Ok((cache, backend))
}

/// Execute a database connection with retry logic
async fn connect_with_retry<F, Fut, T>(
    mut connect_fn: F,
    retry_config: &crate::config::RetryConfig,
    db_type: &str,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    if !retry_config.enabled {
        debug!(db_type, "Retry disabled, attempting single connection");
        return connect_fn().await;
    }

    let mut attempt = 0;
    let mut backoff_ms = retry_config.initial_backoff_ms;
    let max_backoff_ms = retry_config.max_backoff_secs * 1000;

    loop {
        attempt += 1;

        match connect_fn().await {
            Ok(result) => {
                if attempt > 1 {
                    info!(
                        attempts = attempt,
                        db_type, "Database connection succeeded after retry"
                    );
                }
                return Ok(result);
            }
            Err(err) => {
                // Check if error is retryable
                if !is_retryable_error(&err) {
                    error!(
                        attempts = attempt,
                        db_type,
                        error = %err,
                        "Database connection failed with non-retryable error"
                    );
                    return Err(err);
                }

                // Check if we've exhausted retries
                if attempt >= retry_config.max_attempts {
                    error!(
                        attempts = attempt,
                        db_type,
                        error = %err,
                        "Database connection failed after max retries"
                    );
                    return Err(err);
                }

                // Log retry attempt
                warn!(
                    attempt,
                    max_attempts = retry_config.max_attempts,
                    backoff_ms,
                    db_type,
                    error = %err,
                    "Database connection failed, retrying"
                );

                // Wait before retry
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;

                // Calculate next backoff
                backoff_ms = match retry_config.backoff_strategy {
                    BackoffStrategy::Exponential => (backoff_ms * 2).min(max_backoff_ms),
                    BackoffStrategy::Fibonacci => {
                        // Simple fibonacci approximation: next = current * 1.618
                        ((backoff_ms as f64 * 1.618) as u64).min(max_backoff_ms)
                    }
                    BackoffStrategy::Constant => retry_config.initial_backoff_ms,
                };
            }
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
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn test_retryable_connection_error() {
        let err = anyhow!("Connection refused");
        assert!(is_retryable_error(&err));
    }

    #[test]
    fn test_retryable_timeout_error() {
        let err = anyhow!("Connection timeout");
        assert!(is_retryable_error(&err));
    }

    #[test]
    fn test_retryable_busy_error() {
        let err = anyhow!("Database is busy");
        assert!(is_retryable_error(&err));
    }

    #[test]
    fn test_retryable_locked_error() {
        let err = anyhow!("Database table is locked");
        assert!(is_retryable_error(&err));
    }

    #[test]
    fn test_non_retryable_auth_error() {
        let err = anyhow!("Authentication failed");
        assert!(!is_retryable_error(&err));
    }

    #[test]
    fn test_non_retryable_permission_error() {
        let err = anyhow!("Permission denied");
        assert!(!is_retryable_error(&err));
    }

    #[test]
    fn test_non_retryable_invalid_error() {
        let err = anyhow!("Invalid database URL");
        assert!(!is_retryable_error(&err));
    }

    #[test]
    fn test_non_retryable_syntax_error() {
        let err = anyhow!("SQL syntax error");
        assert!(!is_retryable_error(&err));
    }

    #[test]
    fn test_unknown_error_defaults_to_retryable() {
        let err = anyhow!("Some random error");
        assert!(is_retryable_error(&err));
    }

    #[tokio::test]
    async fn test_retry_disabled() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let retry_config = crate::config::RetryConfig {
            enabled: false,
            max_attempts: 3,
            initial_backoff_ms: 100,
            max_backoff_secs: 2,
            backoff_strategy: BackoffStrategy::Exponential,
        };

        let call_count = Arc::new(AtomicU32::new(0));
        let count_clone = call_count.clone();
        let result = connect_with_retry(
            move || {
                let count_clone = count_clone.clone();
                async move {
                    count_clone.fetch_add(1, Ordering::SeqCst);
                    Ok::<i32, anyhow::Error>(42)
                }
            },
            &retry_config,
            "test",
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_retry_succeeds_first_attempt() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let retry_config = crate::config::RetryConfig {
            enabled: true,
            max_attempts: 3,
            initial_backoff_ms: 10,
            max_backoff_secs: 1,
            backoff_strategy: BackoffStrategy::Exponential,
        };

        let call_count = Arc::new(AtomicU32::new(0));
        let count_clone = call_count.clone();
        let result = connect_with_retry(
            move || {
                let count_clone = count_clone.clone();
                async move {
                    count_clone.fetch_add(1, Ordering::SeqCst);
                    Ok::<i32, anyhow::Error>(42)
                }
            },
            &retry_config,
            "test",
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_retry_succeeds_after_retries() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let retry_config = crate::config::RetryConfig {
            enabled: true,
            max_attempts: 3,
            initial_backoff_ms: 10,
            max_backoff_secs: 1,
            backoff_strategy: BackoffStrategy::Exponential,
        };

        let call_count = Arc::new(AtomicU32::new(0));
        let count_clone = call_count.clone();
        let result = connect_with_retry(
            move || {
                let count_clone = count_clone.clone();
                async move {
                    let count = count_clone.fetch_add(1, Ordering::SeqCst) + 1;
                    if count < 3 {
                        Err(anyhow!("Connection refused"))
                    } else {
                        Ok::<i32, anyhow::Error>(42)
                    }
                }
            },
            &retry_config,
            "test",
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_fails_after_max_attempts() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let retry_config = crate::config::RetryConfig {
            enabled: true,
            max_attempts: 3,
            initial_backoff_ms: 10,
            max_backoff_secs: 1,
            backoff_strategy: BackoffStrategy::Exponential,
        };

        let call_count = Arc::new(AtomicU32::new(0));
        let count_clone = call_count.clone();
        let result = connect_with_retry(
            move || {
                let count_clone = count_clone.clone();
                async move {
                    count_clone.fetch_add(1, Ordering::SeqCst);
                    Err::<i32, anyhow::Error>(anyhow!("Connection refused"))
                }
            },
            &retry_config,
            "test",
        )
        .await;

        assert!(result.is_err());
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_stops_on_non_retryable_error() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let retry_config = crate::config::RetryConfig {
            enabled: true,
            max_attempts: 3,
            initial_backoff_ms: 10,
            max_backoff_secs: 1,
            backoff_strategy: BackoffStrategy::Exponential,
        };

        let call_count = Arc::new(AtomicU32::new(0));
        let count_clone = call_count.clone();
        let result = connect_with_retry(
            move || {
                let count_clone = count_clone.clone();
                async move {
                    count_clone.fetch_add(1, Ordering::SeqCst);
                    Err::<i32, anyhow::Error>(anyhow!("Authentication failed"))
                }
            },
            &retry_config,
            "test",
        )
        .await;

        assert!(result.is_err());
        assert_eq!(call_count.load(Ordering::SeqCst), 1); // Should not retry
    }
}
