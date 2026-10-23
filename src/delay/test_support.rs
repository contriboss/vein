//! Fixtures for the delay's tests: a policy that is on, without the weekend
//! and release-hour rules (their math is the adapter's, tested there), and
//! a database to record holds in.

use jiff::{SignedDuration, Timestamp};
use vein_adapter::CacheBackend;

use crate::config::DelayPolicyConfig;

/// On, 3 days, no weekend or release-hour shifts.
pub fn policy() -> DelayPolicyConfig {
    DelayPolicyConfig {
        enabled: true,
        default_delay_days: 3,
        skip_weekends: false,
        business_hours_only: false,
        ..DelayPolicyConfig::default()
    }
}

/// `hours` ago, as RFC 3339.
pub fn hours_ago(hours: i64) -> String {
    (Timestamp::now() - SignedDuration::from_hours(hours)).to_string()
}

pub async fn index() -> CacheBackend {
    vein_adapter::test_support::connect_test_backend()
        .await
        .unwrap()
}
