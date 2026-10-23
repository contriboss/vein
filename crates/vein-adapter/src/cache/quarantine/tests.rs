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

/// Stored as text in `package_versions.status` and `.kind`: every value
/// reads back as itself.
#[test]
fn statuses_and_kinds_round_trip_through_their_text() {
    for status in [
        VersionStatus::Quarantine,
        VersionStatus::Available,
        VersionStatus::Blocked,
        VersionStatus::Pinned,
    ] {
        assert_eq!(status.to_string().parse(), Ok(status), "{status}");
    }
    assert!(
        "yanked".parse::<VersionStatus>().is_err(),
        "migrated to blocked"
    );

    for kind in [AssetKind::Gem, AssetKind::Crate, AssetKind::NpmPackage] {
        assert_eq!(kind.as_str().parse::<AssetKind>(), Ok(kind), "{kind:?}");
    }
    assert!("pypi".parse::<AssetKind>().is_err());
}
