use super::*;

#[test]
fn parse_compact_index_targets_parses_platform_variants() {
    let targets = parse_compact_index_targets(
        "---\n1.0.0 ruby|abc123|deps\n1.0.0 x86_64-linux|def456|deps\n2.0.0 |ghi789|deps\n",
    );

    assert_eq!(
        targets,
        vec![
            GemSyncTarget::new("1.0.0", None),
            GemSyncTarget::new("1.0.0", Some("x86_64-linux")),
            GemSyncTarget::new("2.0.0", None),
        ]
    );
}

#[test]
fn filter_targets_keeps_all_variants_when_platform_not_requested() {
    let targets = vec![
        GemSyncTarget::new("1.0.0", None),
        GemSyncTarget::new("1.0.0", Some("x86_64-linux")),
        GemSyncTarget::new("1.0.0", Some("x86_64-linux")),
    ];

    assert_eq!(
        filter_targets(targets, None),
        vec![
            GemSyncTarget::new("1.0.0", None),
            GemSyncTarget::new("1.0.0", Some("x86_64-linux")),
        ]
    );
}

#[test]
fn filter_targets_matches_ruby_platform_to_default_artifact() {
    let targets = vec![
        GemSyncTarget::new("1.0.0", None),
        GemSyncTarget::new("1.0.0", Some("java")),
    ];

    assert_eq!(
        filter_targets(targets, Some("ruby")),
        vec![GemSyncTarget::new("1.0.0", None)]
    );
}

#[test]
fn build_url_preserves_base_path() {
    let base = Uri::parse("https://mirror.example.com/rubygems/").unwrap();
    let url = build_url(&base, "/info/rack");

    assert_eq!(
        url.as_str(),
        "https://mirror.example.com/rubygems/info/rack"
    );
}
