use super::*;

#[test]
fn typescript_tags_are_ts_and_a_release() {
    for (tag, expected) in [
        ("ts5.9", true),
        ("ts2.0", true),
        ("ts10.12", true),
        ("old-version", false),
        ("ts", false),
        ("ts5", false),
        ("ts5.", false),
        ("tsx.1", false),
        ("next", false),
    ] {
        assert_eq!(is_typescript_tag(tag), expected, "{tag}");
    }
}

#[test]
fn an_empty_deprecation_message_lifts_it() {
    let packument = Packument::parse(
        br#"{"versions": {
            "1.3.0": {"deprecated": "use String.prototype.padStart()"},
            "1.2.0": {"deprecated": ""},
            "1.1.0": {}
        }}"#,
    )
    .unwrap();
    assert_eq!(
        packument.deprecation("1.3.0"),
        Some("use String.prototype.padStart()")
    );
    assert_eq!(packument.deprecation("1.2.0"), None);
    assert_eq!(packument.deprecation("1.1.0"), None);
    assert_eq!(packument.deprecation("9.9.9"), None);
}
