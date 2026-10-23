//! Release delay tracking for supply chain protection.
//!
//! Every registry's new versions wait out a delay before they appear in
//! the index vein serves, so a malicious release that gets yanked within
//! hours never reaches a client. The policy itself lives in vein; these
//! are the rows it keeps (versions held, released, pinned or blocked) and
//! the availability math.

use jiff::civil::{Time, Weekday};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use crate::AssetKind;

/// Where a tracked version stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VersionStatus {
    /// In its delay: hidden from the index, still downloadable.
    #[default]
    Quarantine,
    /// Delay over, or approved: visible.
    Available,
    /// Blocked by an admin: hidden, and downloads refused.
    Blocked,
    /// Released early by config or an admin (e.g. a security fix).
    Pinned,
}

impl std::fmt::Display for VersionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Quarantine => "quarantine",
            Self::Available => "available",
            Self::Blocked => "blocked",
            Self::Pinned => "pinned",
        })
    }
}

impl std::str::FromStr for VersionStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "quarantine" => Ok(Self::Quarantine),
            "available" => Ok(Self::Available),
            "blocked" => Ok(Self::Blocked),
            "pinned" => Ok(Self::Pinned),
            _ => Err(()),
        }
    }
}

/// A package version the delay policy tracks: one it held back, or one an
/// admin approved, pinned or blocked.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageVersion {
    pub id: i64,
    pub kind: AssetKind,
    pub name: String,
    pub version: String,
    /// Gems only: the native platform (`x86_64-linux`); `None` for `ruby`.
    pub platform: Option<String>,
    /// When the registry published it (or vein first saw it, when the
    /// registry does not say).
    pub published_at: Timestamp,
    /// When the delay ends.
    pub available_after: Timestamp,
    pub status: VersionStatus,
    /// Why it has this status (`auto`, `approved: …`, `blocked: …`).
    pub status_reason: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// Counts across every registry.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuarantineStats {
    pub total_quarantined: u64,
    pub total_available: u64,
    pub total_blocked: u64,
    pub total_pinned: u64,
    pub versions_releasing_today: u64,
    pub versions_releasing_this_week: u64,
}

/// The timing half of vein's delay policy (per-package delays and pins
/// live in vein's config).
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

#[cfg(test)]
mod tests;
