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

#[test]
fn test_delay_for_gem_default() {
    let config = DelayPolicyConfig::default();
    assert_eq!(config.delay_for_gem("rails"), 3);
}

#[test]
fn test_delay_for_gem_override() {
    let config = DelayPolicyConfig {
        gems: vec![GemDelayOverride {
            name: "rails".to_string(),
            delay_days: 7,
            pattern: false,
        }],
        ..Default::default()
    };
    assert_eq!(config.delay_for_gem("rails"), 7);
    assert_eq!(config.delay_for_gem("rack"), 3);
}

#[test]
fn test_delay_for_gem_pattern() {
    let config = DelayPolicyConfig {
        gems: vec![GemDelayOverride {
            name: "*-internal".to_string(),
            delay_days: 0,
            pattern: true,
        }],
        ..Default::default()
    };
    assert_eq!(config.delay_for_gem("my-gem-internal"), 0);
    assert_eq!(config.delay_for_gem("rails"), 3);
}

#[test]
fn test_is_pinned() {
    let config = DelayPolicyConfig {
        pinned: vec![PinnedVersion {
            name: "nokogiri".to_string(),
            version: "1.16.0".to_string(),
            reason: "CVE-2024-XXXXX".to_string(),
        }],
        ..Default::default()
    };
    assert!(config.is_pinned("nokogiri", "1.16.0"));
    assert!(!config.is_pinned("nokogiri", "1.15.0"));
    assert!(!config.is_pinned("rails", "7.0.0"));
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

#[test]
fn test_toml_parsing() {
    let toml = r#"
        enabled = true
        default_delay_days = 5
        skip_weekends = false
        business_hours_only = true
        release_hour_utc = 14

        [[gems]]
        name = "rails"
        delay_days = 7

        [[gems]]
        name = "*-internal"
        delay_days = 0
        pattern = true

        [[pinned]]
        name = "nokogiri"
        version = "1.16.0"
        reason = "CVE-2024-XXXXX"
    "#;

    let config: DelayPolicyConfig = toml::from_str(toml).unwrap();
    assert!(config.enabled);
    assert_eq!(config.default_delay_days, 5);
    assert!(!config.skip_weekends);
    assert_eq!(config.release_hour_utc, 14);
    assert_eq!(config.gems.len(), 2);
    assert_eq!(config.pinned.len(), 1);
}
