use super::*;
use crate::config::BackoffStrategy;

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
        jitter_factor: 1.0,
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
        jitter_factor: 1.0,
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
        jitter_factor: 1.0,
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
        jitter_factor: 1.0,
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
        jitter_factor: 1.0,
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
