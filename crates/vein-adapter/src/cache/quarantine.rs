//! Quarantine types and logic for supply chain protection.
//!
//! Implements a time buffer that delays new gem versions from appearing
//! in the index, protecting against supply chain attacks like malicious
//! gem releases that get yanked within hours.

use chrono::{DateTime, Datelike, Duration, NaiveTime, Utc, Weekday};
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
    pub published_at: DateTime<Utc>,
    /// When this version becomes visible in the index
    pub available_after: DateTime<Utc>,
    pub status: VersionStatus,
    /// Reason for current status (e.g., "auto", "pinned: CVE-2024-XXX")
    pub status_reason: Option<String>,
    /// Whether upstream has yanked this version
    pub upstream_yanked: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Information about a quarantined version, used in HTTP headers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuarantineInfo {
    pub served_version: String,
    pub requested_version: String,
    pub available_after: DateTime<Utc>,
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
/// DateTime when the version should become visible in the index
pub fn calculate_availability(published: DateTime<Utc>, policy: &DelayPolicy) -> DateTime<Utc> {
    let mut available = published + Duration::days(i64::from(policy.default_delay_days));

    if policy.skip_weekends {
        // Push weekend releases to Monday
        match available.weekday() {
            Weekday::Sat => available = available + Duration::days(2),
            Weekday::Sun => available = available + Duration::days(1),
            _ => {}
        }
    }

    if policy.business_hours_only {
        // Set to configured hour (default 9:00 AM UTC)
        if let Some(time) = NaiveTime::from_hms_opt(u32::from(policy.release_hour_utc), 0, 0) {
            available = available.date_naive().and_time(time).and_utc();
        }
    }

    available
}

/// Check if a version is currently available (visible in index).
///
/// # Arguments
/// * `gem_version` - The version to check
/// * `now` - Current time
///
/// # Returns
/// `true` if the version should be visible in index responses
pub fn is_version_available(gem_version: &GemVersion, now: DateTime<Utc>) -> bool {
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
pub fn is_version_downloadable(gem_version: &GemVersion) -> bool {
    gem_version.status != VersionStatus::Yanked
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Timelike};

    #[test]
    fn test_calculate_availability_basic() {
        let published = Utc.with_ymd_and_hms(2025, 1, 6, 14, 0, 0).unwrap(); // Monday
        let policy = DelayPolicy {
            default_delay_days: 3,
            skip_weekends: false,
            business_hours_only: false,
            release_hour_utc: 9,
        };

        let available = calculate_availability(published, &policy);
        assert_eq!(available.weekday(), Weekday::Thu);
    }

    #[test]
    fn test_calculate_availability_skip_weekends() {
        // Thursday + 3 days = Sunday → should push to Monday
        let published = Utc.with_ymd_and_hms(2025, 1, 9, 14, 0, 0).unwrap(); // Thursday
        let policy = DelayPolicy {
            default_delay_days: 3,
            skip_weekends: true,
            business_hours_only: false,
            release_hour_utc: 9,
        };

        let available = calculate_availability(published, &policy);
        assert_eq!(available.weekday(), Weekday::Mon);
    }

    #[test]
    fn test_calculate_availability_business_hours() {
        let published = Utc.with_ymd_and_hms(2025, 1, 6, 22, 0, 0).unwrap(); // Monday 10pm
        let policy = DelayPolicy {
            default_delay_days: 3,
            skip_weekends: false,
            business_hours_only: true,
            release_hour_utc: 9,
        };

        let available = calculate_availability(published, &policy);
        assert_eq!(available.hour(), 9);
    }

    #[test]
    fn test_is_version_available() {
        let now = Utc::now();

        // Quarantined, not yet available
        let quarantined = GemVersion {
            id: 1,
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            platform: None,
            sha256: None,
            published_at: now - Duration::days(1),
            available_after: now + Duration::days(2),
            status: VersionStatus::Quarantine,
            status_reason: None,
            upstream_yanked: false,
            created_at: now,
            updated_at: now,
        };
        assert!(!is_version_available(&quarantined, now));

        // Quarantined but time has passed
        let expired_quarantine = GemVersion {
            available_after: now - Duration::hours(1),
            ..quarantined.clone()
        };
        assert!(is_version_available(&expired_quarantine, now));

        // Available status
        let available = GemVersion {
            status: VersionStatus::Available,
            ..quarantined.clone()
        };
        assert!(is_version_available(&available, now));

        // Pinned status
        let pinned = GemVersion {
            status: VersionStatus::Pinned,
            ..quarantined.clone()
        };
        assert!(is_version_available(&pinned, now));

        // Yanked
        let yanked = GemVersion {
            status: VersionStatus::Yanked,
            ..quarantined.clone()
        };
        assert!(!is_version_available(&yanked, now));
    }

    #[test]
    fn test_is_version_downloadable() {
        let now = Utc::now();

        let base = GemVersion {
            id: 1,
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            platform: None,
            sha256: None,
            published_at: now,
            available_after: now + Duration::days(3),
            status: VersionStatus::Quarantine,
            status_reason: None,
            upstream_yanked: false,
            created_at: now,
            updated_at: now,
        };

        // Quarantined versions can still be downloaded
        assert!(is_version_downloadable(&base));

        // Yanked versions cannot be downloaded
        let yanked = GemVersion {
            status: VersionStatus::Yanked,
            ..base
        };
        assert!(!is_version_downloadable(&yanked));
    }

    #[test]
    fn test_version_status_display() {
        assert_eq!(VersionStatus::Quarantine.to_string(), "quarantine");
        assert_eq!(VersionStatus::Available.to_string(), "available");
        assert_eq!(VersionStatus::Yanked.to_string(), "yanked");
        assert_eq!(VersionStatus::Pinned.to_string(), "pinned");
    }

    #[test]
    fn test_version_status_from_str() {
        assert_eq!(
            "quarantine".parse::<VersionStatus>(),
            Ok(VersionStatus::Quarantine)
        );
        assert_eq!(
            "available".parse::<VersionStatus>(),
            Ok(VersionStatus::Available)
        );
        assert_eq!("yanked".parse::<VersionStatus>(), Ok(VersionStatus::Yanked));
        assert_eq!("pinned".parse::<VersionStatus>(), Ok(VersionStatus::Pinned));
        assert!("invalid".parse::<VersionStatus>().is_err());
    }
}
