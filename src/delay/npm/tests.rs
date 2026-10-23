use serde_json::json;

use super::super::test_support::{hours_ago, index, policy};
use super::*;

/// typescript's real dist-tags and versions, abridged.
fn typescript() -> Map<String, Value> {
    let versions = [
        "6.0.3",
        "7.0.1-rc",
        "7.0.2",
        "7.1.0-beta",
        "7.1.0-dev.20261023.1",
    ];
    json!({
        "name": "typescript",
        "dist-tags": {
            "latest": "7.0.2",
            "rc": "7.0.1-rc",
            "beta": "7.1.0-beta",
            "next": "7.1.0-dev.20261023.1",
        },
        "versions": versions.iter().map(|v| (v.to_string(), json!({}))).collect::<Map<_, _>>(),
        "time": versions.iter().map(|v| (v.to_string(), json!("2026-10-01T00:00:00Z")))
            .chain([("modified".to_string(), json!("2026-10-23T00:00:00Z"))])
            .collect::<Map<_, _>>(),
    })
    .as_object()
    .unwrap()
    .clone()
}

#[test]
fn hidden_versions_take_their_tags_with_them() {
    let mut document = typescript();
    // MOCKED: as if 7.0.2 and the nightly were still in their delay.
    remove_versions(
        &mut document,
        &["7.0.2".into(), "7.1.0-dev.20261023.1".into()],
    );

    assert_eq!(
        document["dist-tags"],
        json!({"latest": "6.0.3", "rc": "7.0.1-rc", "beta": "7.1.0-beta"}),
        "latest falls back to the newest release left; next goes"
    );
    let versions: Vec<&String> = document["versions"].as_object().unwrap().keys().collect();
    assert_eq!(versions, ["6.0.3", "7.0.1-rc", "7.1.0-beta"]);
    assert!(document["time"].get("7.0.2").is_none());
    assert!(document["time"].get("modified").is_some(), "not a version");
}

#[test]
fn without_a_release_left_latest_goes() {
    let mut document = typescript();
    remove_versions(&mut document, &["6.0.3".into(), "7.0.2".into()]);
    assert!(document["dist-tags"].get("latest").is_none());
    assert_eq!(document["dist-tags"]["beta"], "7.1.0-beta");
}

#[tokio::test]
async fn fresh_versions_leave_the_packument() {
    // Real: left-pad 1.3.0's publish time. MOCKED: a 1.4.0 an hour old.
    let mut packument = json!({
        "name": "left-pad",
        "dist-tags": {"latest": "1.4.0"},
        "versions": {"1.3.0": {}, "1.4.0": {}},
        "time": {"1.3.0": "2018-04-09T01:10:45.796Z", "1.4.0": hours_ago(1)},
    });
    filter_packument(&policy(), &index().await, "left-pad", &mut packument).await;
    assert_eq!(packument["dist-tags"]["latest"], "1.3.0");
    assert!(packument["versions"].get("1.4.0").is_none());
    assert!(packument["versions"].get("1.3.0").is_some());
}
