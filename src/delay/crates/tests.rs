use super::super::test_support::{hours_ago, index, policy};
use super::*;

/// Real entries from crates.io's sparse index for `anyhow`, deps abridged.
const ANYHOW_1_0_104: &str = r#"{"name":"anyhow","vers":"1.0.104","deps":[],"cksum":"ee","features":{},"yanked":false,"pubtime":"2026-07-18T20:59:37Z"}
"#;

#[test]
fn entries_give_version_and_publish_time() {
    assert_eq!(
        parse_line(ANYHOW_1_0_104),
        Some(Listed {
            version: "1.0.104",
            platform: None,
            published_at: Some("2026-07-18T20:59:37Z".parse().unwrap()),
        })
    );
    let undated = r#"{"name":"anyhow","vers":"1.0.0","deps":[],"cksum":"aa","features":{}}"#;
    assert_eq!(
        parse_line(undated).map(|listed| listed.published_at),
        Some(None)
    );
    assert_eq!(parse_line("not json\n"), None);
}

#[tokio::test]
async fn fresh_publishes_leave_the_index_file() {
    // MOCKED: anyhow 1.0.105 is not out; it stands in for a publish an hour old.
    let fresh = format!(
        "{{\"name\":\"anyhow\",\"vers\":\"1.0.105\",\"deps\":[],\"cksum\":\"ff\",\"features\":{{}},\"yanked\":false,\"pubtime\":\"{}\"}}\n",
        hours_ago(1)
    );
    let file = format!("{ANYHOW_1_0_104}{fresh}");
    let served = filter_index(&policy(), &index().await, "anyhow", file.into_bytes()).await;
    assert_eq!(String::from_utf8(served).unwrap(), ANYHOW_1_0_104);
}
