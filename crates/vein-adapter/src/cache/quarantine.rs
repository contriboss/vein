//! Quarantine types and logic for supply chain protection.
//!
//! Implements a time buffer that delays new gem versions from appearing
//! in the index, protecting against supply chain attacks like malicious
//! gem releases that get yanked within hours.

use jiff::civil::{Time, Weekday};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

/// Status of a gem version in the quarantine system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VersionStatus {
    /// In delay period - hidden from index but downloadable directly
    #[default]
    Quarantine,
    /// Delay expired - visible in index and downloadable
    Available,
    /// Upstream removed it - hidden and blocked
    Yanked,
    /// Manual override - immediately available (e.g., critical security patch)
    Pinned,
}

impl std::fmt::Display for VersionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Quarantine => write!(f, "quarantine"),
            Self::Available => write!(f, "available"),
            Self::Yanked => write!(f, "yanked"),
            Self::Pinned => write!(f, "pinned"),
        }
    }
}

impl std::str::FromStr for VersionStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "quarantine" => Ok(Self::Quarantine),
            "available" => Ok(Self::Available),
            "yanked" => Ok(Self::Yanked),
            "pinned" => Ok(Self::Pinned),
            _ => Err(()),
        }
    }
}

/// A gem version with quarantine tracking information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GemVersion {
    pub id: i64,
    pub name: String,
    pub version: String,
    pub platform: Option<String>,
    pub sha256: Option<String>,
    /// When this version was first seen/published
    pub published_at: Timestamp,
    /// When this version becomes visible in the index
    pub available_after: Timestamp,
    pub status: VersionStatus,
    /// Reason for current status (e.g., "auto", "pinned: CVE-2024-XXX")
    pub status_reason: Option<String>,
    /// Whether upstream has yanked this version
    pub upstream_yanked: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// Information about a quarantined version, used in HTTP headers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuarantineInfo {
    pub served_version: String,
    pub requested_version: String,
    pub available_after: Timestamp,
    pub reason: String,
    pub quarantined_versions: Vec<String>,
}

/// Statistics about the quarantine system.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuarantineStats {
    pub total_quarantined: u64,
    pub total_available: u64,
    pub total_yanked: u64,
    pub total_pinned: u64,
    pub versions_releasing_today: u64,
    pub versions_releasing_this_week: u64,
}

/// Policy configuration for delay calculation.
/// This is a simplified version for the adapter crate - full config lives in main crate.
#[derive(Debug, Clone)]
pub struct DelayPolicy {
    pub default_delay_days: u32,
    pub skip_weekends: bool,
    pub business_hours_only: bool,
    pub release_hour_utc: u8,
}

impl Default for DelayPolicy {
    fn default() -> Self {
        Self {
            default_delay_days: 3,
            skip_weekends: true,
            business_hours_only: true,
            release_hour_utc: 9,
        }
    }
}

/// Calculate when a version should become available based on policy.
///
/// # Arguments
/// * `published` - When the version was first seen
/// * `policy` - Delay policy configuration
///
/// # Returns
/// `DateTime` when the version should become visible in the index
#[must_use]
pub fn calculate_availability(published: Timestamp, policy: &DelayPolicy) -> Timestamp {
    let mut available = after_days(published, i64::from(policy.default_delay_days));

    if policy.skip_weekends {
        // Push weekend releases to Monday
        match available.to_zoned(TimeZone::UTC).weekday() {
            Weekday::Saturday => available = after_days(available, 2),
            Weekday::Sunday => available = after_days(available, 1),
            _ => {}
        }
    }

    // Set to the configured hour (default 9:00 UTC); vein's config
    // validation refuses an hour past 23, which is left alone here.
    if policy.business_hours_only
        && let Ok(hour) = i8::try_from(policy.release_hour_utc)
        && let Ok(time) = Time::new(hour, 0, 0, 0)
        && let Ok(at) =
            TimeZone::UTC.to_timestamp(available.to_zoned(TimeZone::UTC).date().to_datetime(time))
    {
        available = at;
    }

    available
}

/// `days` (of 24 hours, as every UTC day is) after `ts`, saturating at the
/// end of time.
fn after_days(ts: Timestamp, days: i64) -> Timestamp {
    ts.saturating_add(SignedDuration::from_hours(days.saturating_mul(24)))
        .unwrap_or(Timestamp::MAX)
}

/// Check if a version is currently available (visible in index).
///
/// # Arguments
/// * `gem_version` - The version to check
/// * `now` - Current time
///
/// # Returns
/// `true` if the version should be visible in index responses
#[must_use]
pub fn is_version_available(gem_version: &GemVersion, now: Timestamp) -> bool {
    match gem_version.status {
        VersionStatus::Available | VersionStatus::Pinned => true,
        VersionStatus::Yanked => false,
        VersionStatus::Quarantine => now >= gem_version.available_after,
    }
}

/// Check if a version should be served for direct download.
/// Note: Even quarantined versions can be downloaded directly.
///
/// # Arguments
/// * `gem_version` - The version to check
///
/// # Returns
/// `true` if the version can be downloaded (only yanked versions are blocked)
#[must_use]
pub fn is_version_downloadable(gem_version: &GemVersion) -> bool {
    gem_version.status != VersionStatus::Yanked
}

#[cfg(test)]
mod tests;
