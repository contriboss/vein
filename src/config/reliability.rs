use chrono_machines::{BackoffPolicy, ConstantBackoff, ExponentialBackoff, FibonacciBackoff};
use serde::Deserialize;

// Retry configuration only (circuit breaker removed)

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BackoffStrategy {
    #[default]
    Exponential,
    Fibonacci,
    Constant,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RetryConfig {
    /// Enable retry mechanism
    #[serde(default = "RetryConfig::default_enabled")]
    pub enabled: bool,
    /// Maximum number of retry attempts
    #[serde(default = "RetryConfig::default_max_attempts")]
    pub max_attempts: u32,
    /// Initial backoff duration (milliseconds)
    #[serde(default = "RetryConfig::default_initial_backoff_ms")]
    pub initial_backoff_ms: u64,
    /// Maximum backoff duration (seconds)
    #[serde(default = "RetryConfig::default_max_backoff_secs")]
    pub max_backoff_secs: u64,
    /// Backoff strategy
    #[serde(default)]
    pub backoff_strategy: BackoffStrategy,
    /// Jitter factor (0.0 = no jitter, 1.0 = full jitter)
    #[serde(default = "RetryConfig::default_jitter_factor")]
    pub jitter_factor: f64,
}

impl RetryConfig {
    const fn default_enabled() -> bool {
        true
    }

    const fn default_max_attempts() -> u32 {
        3
    }

    const fn default_initial_backoff_ms() -> u64 {
        100
    }

    const fn default_max_backoff_secs() -> u64 {
        2
    }

    const fn default_jitter_factor() -> f64 {
        1.0 // Full jitter by default (prevents thundering herd)
    }

    /// Total attempts per operation, first try included. A disabled config
    /// still makes one attempt; values past `u8::MAX` saturate rather than wrap.
    #[must_use]
    pub fn attempts(&self) -> u8 {
        if self.enabled {
            u8::try_from(self.max_attempts).unwrap_or(u8::MAX)
        } else {
            1
        }
    }

    /// The chrono-machines policy this config describes.
    #[must_use]
    pub fn backoff_policy(&self) -> BackoffPolicy {
        let attempts = self.attempts();
        let max_delay_ms = self.max_backoff_secs.saturating_mul(1000);
        match self.backoff_strategy {
            BackoffStrategy::Exponential => ExponentialBackoff::new()
                .base_delay_ms(self.initial_backoff_ms)
                .max_delay_ms(max_delay_ms)
                .max_attempts(attempts)
                .jitter_factor(self.jitter_factor)
                .into(),
            BackoffStrategy::Fibonacci => FibonacciBackoff::new()
                .base_delay_ms(self.initial_backoff_ms)
                .max_delay_ms(max_delay_ms)
                .max_attempts(attempts)
                .jitter_factor(self.jitter_factor)
                .into(),
            BackoffStrategy::Constant => ConstantBackoff::new()
                .delay_ms(self.initial_backoff_ms)
                .max_attempts(attempts)
                .jitter_factor(self.jitter_factor)
                .into(),
        }
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            enabled: Self::default_enabled(),
            max_attempts: Self::default_max_attempts(),
            initial_backoff_ms: Self::default_initial_backoff_ms(),
            max_backoff_secs: Self::default_max_backoff_secs(),
            backoff_strategy: BackoffStrategy::default(),
            jitter_factor: Self::default_jitter_factor(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReliabilityConfig {
    #[serde(default)]
    pub retry: RetryConfig,
}
