use crate::config::reliability::{BackoffStrategy, ReliabilityConfig, RetryConfig};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct HotCacheConfig {
    /// Cron schedule for refreshing hot cache (e.g., "0 0 * * * *" = hourly)
    /// Set to empty string to disable automatic refresh
    #[serde(default = "HotCacheConfig::default_refresh_schedule")]
    pub refresh_schedule: String,
    #[serde(default = "HotCacheConfig::default_reliability")]
    pub reliability: ReliabilityConfig,
}

impl HotCacheConfig {
    fn default_refresh_schedule() -> String {
        // Every hour at :00
        "0 0 * * * *".to_string()
    }

    fn default_reliability() -> ReliabilityConfig {
        ReliabilityConfig {
            retry: RetryConfig {
                max_attempts: 3,
                initial_backoff_ms: 1000,
                backoff_strategy: BackoffStrategy::Constant,
                ..RetryConfig::default()
            },
        }
    }
}

impl Default for HotCacheConfig {
    fn default() -> Self {
        Self {
            refresh_schedule: Self::default_refresh_schedule(),
            reliability: Self::default_reliability(),
        }
    }
}
