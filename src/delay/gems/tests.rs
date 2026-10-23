use super::super::test_support::{hours_ago, index, policy};
use super::*;

/// Real lines from rubygems.org's `info/rails` and `info/nokogiri`.
const RAILS_8_1_4: &str = "8.1.4 actioncable:= 8.1.4,railties:= 8.1.4|checksum:5f3ecdf9,ruby:>= 3.2.0,rubygems:>= 1.8.11,created_at:2026-09-24T14:22:14Z\n";
const NOKOGIRI_NATIVE: &str = "1.18.10-x86_64-linux-gnu racc:~> 1.4|checksum:ff5ba26b,ruby:< 3.5.dev&>= 3.1,rubygems:>= 3.3.22,created_at:2025-09-15T01:53:50Z\n";

#[test]
fn lines_give_version_platform_and_publish_time() {
    let at = |rfc3339: &str| Some(rfc3339.parse::<Timestamp>().unwrap());
    let cases = [
        (
            RAILS_8_1_4,
            Some(Listed {
                version: "8.1.4",
                platform: None,
                published_at: at("2026-09-24T14:22:14Z"),
            }),
        ),
        (
            NOKOGIRI_NATIVE,
            Some(Listed {
                version: "1.18.10",
                platform: Some("x86_64-linux-gnu"),
                published_at: at("2025-09-15T01:53:50Z"),
            }),
        ),
        // A mirror that does not date its lines.
        (
            "3.2.7 |checksum:93e1,ruby:>= 2.4.0\n",
            Some(Listed {
                version: "3.2.7",
                platform: None,
                published_at: None,
            }),
        ),
        ("---\n", None),
        ("\n", None),
    ];
    for (line, expected) in cases {
        assert_eq!(parse_line(line), expected, "{line:?}");
    }
}

/// A release an hour old is held; the rest of the file comes back byte for
/// byte, header and trailing newline included.
#[tokio::test]
async fn fresh_releases_leave_the_info_file() {
    // MOCKED: rails 8.1.5 is not out; it stands in for a release an hour old.
    let fresh = format!(
        "8.1.5 railties:= 8.1.5|checksum:aaaa,ruby:>= 3.2.0,created_at:{}\n",
        hours_ago(1)
    );
    let info = format!("---\n{RAILS_8_1_4}{fresh}");
    let index = index().await;

    let served = filter_info(&policy(), &index, "rails", info.clone().into_bytes()).await;
    assert_eq!(
        String::from_utf8(served).unwrap(),
        format!("---\n{RAILS_8_1_4}")
    );

    let off = DelayPolicyConfig {
        enabled: false,
        ..policy()
    };
    let served = filter_info(&off, &index, "rails", info.clone().into_bytes()).await;
    assert_eq!(
        String::from_utf8(served).unwrap(),
        info,
        "the policy is off"
    );
}
