use jiff::SignedDuration;

use super::test_support::{index, policy};
use super::*;
use crate::config::delay_policy::{PackageDelayOverride, PinnedVersion};

fn row(status: VersionStatus, published_at: Timestamp) -> PackageVersion {
    PackageVersion {
        id: 1,
        kind: AssetKind::Gem,
        name: "rails".into(),
        version: "8.1.5".into(),
        platform: None,
        published_at,
        available_after: published_at,
        status,
        status_reason: None,
        created_at: published_at,
        updated_at: published_at,
    }
}

/// (what, registry, name, listed, tracked row, verdict)
type Case<'a> = (
    &'a str,
    AssetKind,
    &'a str,
    Listed<'a>,
    Option<PackageVersion>,
    Verdict,
);

#[test]
fn verdicts_follow_publish_time_overrides_and_decisions() {
    let now: Timestamp = "2026-10-23T12:00:00Z".parse().unwrap();
    let ago = |hours| now - SignedDuration::from_hours(hours);
    let config = DelayPolicyConfig {
        packages: vec![
            PackageDelayOverride {
                registry: None,
                name: "rails".into(),
                names: Vec::new(),
                delay_days: 7,
                pattern: false,
            },
            PackageDelayOverride {
                registry: Some(AssetKind::NpmPackage),
                name: "@my-org/*".into(),
                names: Vec::new(),
                delay_days: 0,
                pattern: true,
            },
        ],
        pinned: vec![PinnedVersion {
            registry: Some(AssetKind::Crate),
            name: "tokio".into(),
            version: "1.54.2".into(),
            reason: "security fix".into(),
        }],
        ..policy()
    };
    let listed = |version, published_at| Listed {
        version,
        platform: None,
        published_at,
    };
    let held_until = |published: Timestamp, days: i64| {
        Verdict::Held(published + SignedDuration::from_hours(days * 24))
    };

    let cases: &[Case<'_>] = &[
        (
            "a day old",
            AssetKind::Crate,
            "serde",
            listed("1.0.229", Some(ago(24))),
            None,
            held_until(ago(24), 3),
        ),
        (
            "past its delay",
            AssetKind::Crate,
            "serde",
            listed("1.0.228", Some(ago(96))),
            None,
            Verdict::Visible,
        ),
        // Per-package delays reach the timing (they once only meant "0 or default").
        (
            "rails waits 7 days",
            AssetKind::Gem,
            "rails",
            listed("8.1.5", Some(ago(96))),
            None,
            held_until(ago(96), 7),
        ),
        (
            "zero delay",
            AssetKind::NpmPackage,
            "@my-org/ui",
            listed("2.0.0", Some(ago(1))),
            None,
            Verdict::Visible,
        ),
        (
            "config pin",
            AssetKind::Crate,
            "tokio",
            listed("1.54.2", Some(ago(1))),
            None,
            Verdict::Visible,
        ),
        (
            "pin is per registry",
            AssetKind::NpmPackage,
            "tokio",
            listed("1.54.2", Some(ago(1))),
            None,
            held_until(ago(1), 3),
        ),
        (
            "undated, untracked",
            AssetKind::Crate,
            "anyhow",
            listed("1.0.0", None),
            None,
            Verdict::Visible,
        ),
        (
            "undated, held before",
            AssetKind::Gem,
            "rails",
            listed("8.1.5", None),
            Some(row(VersionStatus::Quarantine, ago(24))),
            held_until(ago(24), 7),
        ),
        (
            "blocked stays blocked",
            AssetKind::Gem,
            "rails",
            listed("8.1.5", Some(ago(9000))),
            Some(row(VersionStatus::Blocked, ago(9000))),
            Verdict::Blocked,
        ),
        (
            "approved early",
            AssetKind::Gem,
            "rails",
            listed("8.1.5", Some(ago(1))),
            Some(row(VersionStatus::Pinned, ago(1))),
            Verdict::Visible,
        ),
    ];
    for (case, kind, name, listed, row, expected) in cases {
        assert_eq!(
            decide(&config, *kind, name, *listed, row.as_ref(), now),
            *expected,
            "{case}"
        );
    }
}

/// Blocking refuses the download of that exact version; holding does not.
#[tokio::test]
async fn blocked_versions_are_refused_and_held_ones_are_not() {
    let index = index().await;
    let config = policy();
    let key = |version| AssetKey {
        kind: AssetKind::NpmPackage,
        name: "lefft-pad",
        version,
        platform: None,
    };
    // MOCKED: a typosquat's versions.
    index
        .set_version_status(
            &key("1.4.0"),
            VersionStatus::Blocked,
            Some("blocked: typosquat".into()),
        )
        .await
        .unwrap();
    let held = [Listed {
        version: "1.3.9",
        platform: None,
        published_at: Some(Timestamp::now()),
    }];
    screen(&config, &index, AssetKind::NpmPackage, "lefft-pad", &held)
        .await
        .unwrap();

    assert!(is_blocked(&config, &index, &key("1.4.0")).await);
    assert!(
        !is_blocked(&config, &index, &key("1.3.9")).await,
        "held, still downloadable"
    );
    let off = DelayPolicyConfig {
        enabled: false,
        ..policy()
    };
    assert!(
        !is_blocked(&off, &index, &key("1.4.0")).await,
        "the policy is off"
    );
    assert_eq!(
        screen(&off, &index, AssetKind::NpmPackage, "lefft-pad", &held).await,
        None
    );
}
