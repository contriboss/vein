use super::*;

fn branch_entry(
    name: &str,
    status: &str,
    security_maintenance_date: Option<&str>,
    eol_date: Option<&str>,
    expected_eol_date: Option<&str>,
) -> BranchEntry {
    BranchEntry {
        name: name.to_string(),
        status: status.to_string(),
        _date: None,
        security_maintenance_date: security_maintenance_date.map(str::to_string),
        eol_date: eol_date.map(str::to_string),
        expected_eol_date: expected_eol_date.map(str::to_string),
    }
}

fn release_entry(version: &str, date: &str) -> ReleaseEntry {
    ReleaseEntry {
        version: version.to_string(),
        date: date.to_string(),
    }
}

#[test]
fn partition_branches_sorts_security_and_limits_recent_eol() {
    let (security, recent_eol) = partition_branches(vec![
        branch_entry(
            "3.1",
            "security maintenance",
            Some("2026-04-01"),
            None,
            Some("2026-05-01"),
        ),
        branch_entry(
            "3.0",
            "security maintenance",
            Some("2026-03-01"),
            None,
            Some("2026-04-01"),
        ),
        branch_entry("2.7", "eol", None, Some("2025-01-01"), None),
        branch_entry("2.6", "eol", None, Some("2024-01-01"), None),
        branch_entry("2.5", "eol", None, Some("2023-01-01"), None),
        branch_entry("2.4", "eol", None, Some("2022-01-01"), None),
        branch_entry("3.2", "normal maintenance", None, None, None),
    ]);

    assert_eq!(
        security
            .iter()
            .map(|branch| branch.name.as_str())
            .collect::<Vec<_>>(),
        vec!["3.0", "3.1"]
    );
    assert_eq!(
        recent_eol
            .iter()
            .map(|branch| branch.name.as_str())
            .collect::<Vec<_>>(),
        vec!["2.7", "2.6", "2.5"]
    );
}

#[test]
fn latest_release_returns_first_parseable_release() {
    let latest = latest_release(vec![
        release_entry("3.4.1", "invalid"),
        release_entry("3.4.0", "2026-01-15"),
        release_entry("3.3.9", "2025-12-25"),
    ])
    .unwrap();

    assert_eq!(latest.version, "3.4.0");
    assert_eq!(latest.date, NaiveDate::from_ymd_opt(2026, 1, 15).unwrap());
}

#[test]
fn ruby_status_from_sources_builds_expected_snapshot() {
    let status = RubyStatus::from_sources(
        vec![
            branch_entry(
                "3.0",
                "security maintenance",
                Some("2026-03-01"),
                None,
                Some("2026-04-01"),
            ),
            branch_entry("2.7", "eol", None, Some("2025-01-01"), None),
        ],
        vec![release_entry("3.4.0", "2026-01-15")],
    );

    assert_eq!(
        status
            .latest_release
            .as_ref()
            .map(|release| release.version.as_str()),
        Some("3.4.0")
    );
    assert_eq!(status.security_maintenance.len(), 1);
    assert_eq!(status.recent_eol.len(), 1);
}

#[test]
fn backoff_for_attempt_doubles_each_retry() {
    assert_eq!(backoff_for_attempt(1), Duration::from_secs(1));
    assert_eq!(backoff_for_attempt(2), Duration::from_secs(2));
    assert_eq!(backoff_for_attempt(3), Duration::from_secs(4));
}

#[test]
fn should_retry_status_only_retries_retryable_responses() {
    assert!(should_retry_status(StatusCode::TOO_MANY_REQUESTS));
    assert!(should_retry_status(StatusCode::BAD_GATEWAY));
    assert!(!should_retry_status(StatusCode::NOT_FOUND));
}
