use super::*;

#[test]
fn test_default_config() {
    let config = DelayPolicyConfig::default();
    assert!(!config.enabled);
    assert_eq!(config.default_delay_days, 3);
    assert!(config.skip_weekends);
    assert!(config.business_hours_only);
    assert_eq!(config.release_hour_utc, 9);
}

const POLICY: &str = r#"
    enabled = true
    default_delay_days = 3

    [[packages]]
    name = "rails"
    delay_days = 7

    [[packages]]
    registry = "npm"
    name = "@my-org/*"
    pattern = true
    delay_days = 0

    # A publisher's own crates, released at once; first match wins, so
    # before the catch-all below.
    [[packages]]
    registry = "crate"
    names = ["mcp-guest", "stampeded"]
    delay_days = 0

    [[packages]]
    registry = "crates"
    name = "*"
    pattern = true
    delay_days = 1

    [[pinned]]
    registry = "gem"
    name = "nokogiri"
    version = "1.18.10"
    reason = "security fix"
"#;

#[test]
fn overrides_and_pins_follow_registry_and_name() {
    let config: DelayPolicyConfig = toml::from_str(POLICY).unwrap();
    for (kind, name, days) in [
        (AssetKind::Gem, "rails", 7),
        // No registry: the name in every registry.
        (AssetKind::NpmPackage, "rails", 7),
        (AssetKind::NpmPackage, "@my-org/ui", 0),
        (AssetKind::Gem, "@my-org/ui", 3),
        (AssetKind::Crate, "serde", 1),
        (AssetKind::Crate, "mcp-guest", 0),
        (AssetKind::Crate, "stampeded", 0),
        (AssetKind::Gem, "stampeded", 3),
        (AssetKind::Gem, "rack", 3),
    ] {
        assert_eq!(config.delay_days_for(kind, name), days, "{kind:?} {name}");
        assert_eq!(
            config.policy_for(kind, name).default_delay_days,
            days,
            "the timing uses the override, {kind:?} {name}"
        );
    }

    for (kind, version, pinned) in [
        (AssetKind::Gem, "1.18.10", true),
        (AssetKind::Gem, "1.18.9", false),
        (AssetKind::NpmPackage, "1.18.10", false),
    ] {
        assert_eq!(
            config.pin_reason(kind, "nokogiri", version).is_some(),
            pinned,
            "{kind:?} nokogiri {version}"
        );
    }
}

#[test]
fn gem_only_configs_still_parse() {
    let config: DelayPolicyConfig = toml::from_str(
        r#"
        [[gems]]
        name = "rails"
        delay_days = 7
    "#,
    )
    .unwrap();
    assert_eq!(config.delay_days_for(AssetKind::Gem, "rails"), 7);
}

#[test]
fn test_glob_match() {
    // Suffix match
    assert!(glob_match("*-internal", "my-gem-internal"));
    assert!(!glob_match("*-internal", "internal-gem"));

    // Prefix match
    assert!(glob_match("rails-*", "rails-api"));
    assert!(!glob_match("rails-*", "my-rails"));

    // Middle wildcard
    assert!(glob_match("my-*-gem", "my-awesome-gem"));
    assert!(!glob_match("my-*-gem", "your-awesome-gem"));

    // Exact match
    assert!(glob_match("rails", "rails"));
    assert!(!glob_match("rails", "rack"));

    // Wildcard all
    assert!(glob_match("*", "anything"));
}
