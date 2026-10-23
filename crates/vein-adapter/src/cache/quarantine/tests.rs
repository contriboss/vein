use super::*;
fn at(rfc3339: &str) -> Timestamp {
    rfc3339.parse().unwrap()
}

#[test]
fn availability_follows_the_delay_policy() {
    // (published, skip_weekends, business_hours_only, release_hour, expected)
    let cases = [
        // Monday + 3 days = Thursday, same time of day.
        (
            "2025-01-06T14:00:00Z",
            false,
            false,
            9,
            "2025-01-09T14:00:00Z",
        ),
        // Thursday + 3 = Sunday, pushed to Monday.
        (
            "2025-01-09T14:00:00Z",
            true,
            false,
            9,
            "2025-01-13T14:00:00Z",
        ),
        // Wednesday + 3 = Saturday, pushed to Monday, at the release hour.
        (
            "2025-01-08T22:30:00Z",
            true,
            true,
            9,
            "2025-01-13T09:00:00Z",
        ),
        // Monday 22:00 + 3 = Thursday, at the release hour.
        (
            "2025-01-06T22:00:00Z",
            false,
            true,
            9,
            "2025-01-09T09:00:00Z",
        ),
        // Not an hour of the day (config validation refuses it): left alone.
        (
            "2025-01-06T22:00:00Z",
            false,
            true,
            24,
            "2025-01-09T22:00:00Z",
        ),
    ];
    for (published, skip_weekends, business_hours_only, release_hour_utc, expected) in cases {
        let policy = DelayPolicy {
            default_delay_days: 3,
            skip_weekends,
            business_hours_only,
            release_hour_utc,
        };
        assert_eq!(
            calculate_availability(at(published), &policy),
            at(expected),
            "published {published}, {policy:?}"
        );
    }
}

#[test]
fn test_is_version_available() {
    let now = Timestamp::now();

    // Quarantined, not yet available
    let quarantined = GemVersion {
        id: 1,
        name: "test".to_string(),
        version: "1.0.0".to_string(),
        platform: None,
        sha256: None,
        published_at: now - SignedDuration::from_hours(24),
        available_after: now + SignedDuration::from_hours(2 * 24),
        status: VersionStatus::Quarantine,
        status_reason: None,
        upstream_yanked: false,
        created_at: now,
        updated_at: now,
    };
    assert!(!is_version_available(&quarantined, now));

    // Quarantined but time has passed
    let expired_quarantine = GemVersion {
        available_after: now - SignedDuration::from_hours(1),
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
    let now = Timestamp::now();

    let base = GemVersion {
        id: 1,
        name: "test".to_string(),
        version: "1.0.0".to_string(),
        platform: None,
        sha256: None,
        published_at: now,
        available_after: now + SignedDuration::from_hours(3 * 24),
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
