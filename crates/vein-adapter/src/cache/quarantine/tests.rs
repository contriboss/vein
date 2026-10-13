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
        ..quarantined
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
