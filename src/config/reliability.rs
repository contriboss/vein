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
    fn default_enabled() -> bool {
        true
    }

    fn default_max_attempts() -> u32 {
        3
    }

    fn default_initial_backoff_ms() -> u64 {
        100
    }

    fn default_max_backoff_secs() -> u64 {
        2
    }

    fn default_jitter_factor() -> f64 {
        1.0 // Full jitter by default (prevents thundering herd)
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
