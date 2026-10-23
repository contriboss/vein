use super::*;

/// Real lines from rubygems.org's `info/rack` and `info/nokogiri`, in the
/// order they are published (appended, not sorted), checksums abridged.
const INFO: &str = "---\n\
    2.2.9 |checksum:aa,ruby:>= 2.4.0\n\
    3.0.0.rc1 |checksum:bb,ruby:>= 2.4.0\n\
    2.2.24 |checksum:cc,ruby:>= 2.4.0\n\
    1.18.10-x86_64-linux-gnu mini_portile2:~> 2.8.2|checksum:dd,ruby:>= 3.1\n\
    1.18.10 mini_portile2:~> 2.8.2|checksum:ee,ruby:>= 3.1.0\n";

#[test]
fn versions_are_unique_and_newest_first_as_numbers() {
    assert_eq!(
        versions(INFO.as_bytes()),
        ["3.0.0.rc1", "2.2.24", "2.2.9", "1.18.10"]
    );
    assert_eq!(versions(b"---\n"), Vec::<String>::new());
}

#[test]
fn prereleases_have_letters() {
    for (version, expected) in [("3.0.0.rc1", true), ("3.0.0.beta1", true), ("3.2.7", false)] {
        assert_eq!(is_prerelease(version), expected, "{version}");
    }
}

#[test]
fn names_are_checked_before_they_reach_a_path() {
    for (name, expected) in [
        ("rack", true),
        ("aws-sdk-s3", true),
        ("net_http.persistent", true),
        ("../etc/passwd", false),
        ("..", false),
        ("rack?x=1", false),
        ("", false),
    ] {
        assert_eq!(is_valid_name(name), expected, "{name:?}");
    }
}
