use super::*;
use crate::VersionStatus;
use chrono::Utc;

fn gem_version(version: &str) -> GemVersion {
    let now = Utc::now();
    GemVersion {
        id: 0,
        name: "rails".to_string(),
        version: version.to_string(),
        platform: None,
        sha256: None,
        published_at: now,
        available_after: now,
        status: VersionStatus::Available,
        status_reason: None,
        upstream_yanked: false,
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn latest_gem_version_orders_non_semver_versions_numerically() {
    let rows = ["7.1.3.4", "7.1.10", "7.1.9", "7.1.10.rc1"]
        .map(gem_version)
        .to_vec();
    assert_eq!(latest_gem_version(rows).unwrap().version, "7.1.10");
}
