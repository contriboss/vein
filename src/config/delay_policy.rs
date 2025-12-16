//! Delay policy configuration for supply chain protection.
//!
//! Configures the quarantine buffer that delays new gem versions from
//! appearing in the index, protecting against supply chain attacks.

use serde::Deserialize;
use vein_adapter::DelayPolicy as AdapterDelayPolicy;

/// Main delay policy configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DelayPolicyConfig {
    /// Enable the delay policy (opt-in).
    #[serde(default)]
    pub enabled: bool,
    /// Default delay in days for new versions.
    #[serde(default = "DelayPolicyConfig::default_delay_days")]
    pub default_delay_days: u32,
    /// Push releases landing on weekends to Monday.
    #[serde(default = "DelayPolicyConfig::default_skip_weekends")]
    pub skip_weekends: bool,
    /// Only release at business hours (configurable hour).
    #[serde(default = "DelayPolicyConfig::default_business_hours_only")]
    pub business_hours_only: bool,
    /// Hour in UTC for business_hours_only releases (0-23).
    #[serde(default = "DelayPolicyConfig::default_release_hour_utc")]
    pub release_hour_utc: u8,
    /// Per-gem delay overrides.
    #[serde(default)]
    pub gems: Vec<GemDelayOverride>,
    /// Pinned versions (bypass quarantine immediately).
    #[serde(default)]
    pub pinned: Vec<PinnedVersion>,
}

impl DelayPolicyConfig {
    fn default_delay_days() -> u32 {
        3
    }

    fn default_skip_weekends() -> bool {
        true
    }

    fn default_business_hours_only() -> bool {
        true
    }

    fn default_release_hour_utc() -> u8 {
        9
    }

    /// Convert to the adapter's DelayPolicy type.
    pub fn to_adapter_policy(&self) -> AdapterDelayPolicy {
        AdapterDelayPolicy {
            default_delay_days: self.default_delay_days,
            skip_weekends: self.skip_weekends,
            business_hours_only: self.business_hours_only,
            release_hour_utc: self.release_hour_utc,
        }
    }

    /// Get the delay days for a specific gem, considering overrides.
    pub fn delay_for_gem(&self, name: &str) -> u32 {
        for override_config in &self.gems {
            if override_config.pattern {
                if glob_match(&override_config.name, name) {
                    return override_config.delay_days;
                }
            } else if override_config.name == name {
                return override_config.delay_days;
            }
        }
        self.default_delay_days
    }

    /// Check if a specific version is pinned (bypass quarantine).
    pub fn is_pinned(&self, name: &str, version: &str) -> bool {
        self.pinned
            .iter()
            .any(|p| p.name == name && p.version == version)
    }

    /// Get the pin reason if version is pinned.
    pub fn pin_reason(&self, name: &str, version: &str) -> Option<&str> {
        self.pinned
            .iter()
            .find(|p| p.name == name && p.version == version)
            .map(|p| p.reason.as_str())
    }
}

impl Default for DelayPolicyConfig {
    fn default() -> Self {
        Self {
            enabled: false, // Opt-in by default
            default_delay_days: Self::default_delay_days(),
            skip_weekends: Self::default_skip_weekends(),
            business_hours_only: Self::default_business_hours_only(),
            release_hour_utc: Self::default_release_hour_utc(),
            gems: Vec::new(),
            pinned: Vec::new(),
        }
    }
}

/// Per-gem delay override configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct GemDelayOverride {
    /// Gem name (or glob pattern if `pattern` is true).
    pub name: String,
    /// Delay in days for this gem.
    pub delay_days: u32,
    /// If true, `name` is a glob pattern (e.g., "rails-*", "*-internal").
    #[serde(default)]
    pub pattern: bool,
}

/// Pinned version configuration (bypass quarantine).
#[derive(Debug, Clone, Deserialize)]
pub struct PinnedVersion {
    /// Gem name.
    pub name: String,
    /// Version string.
    pub version: String,
    /// Reason for pinning (e.g., "CVE-2024-XXXXX").
    pub reason: String,
}

/// Simple glob matching for patterns like "*-internal" or "rails-*".
fn glob_match(pattern: &str, name: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    if let Some(suffix) = pattern.strip_prefix('*') {
        return name.ends_with(suffix);
    }

    if let Some(prefix) = pattern.strip_suffix('*') {
        return name.starts_with(prefix);
    }

    // If pattern contains * in the middle, do a simple split match
    if let Some(pos) = pattern.find('*') {
        let prefix = &pattern[..pos];
        let suffix = &pattern[pos + 1..];
        return name.starts_with(prefix) && name.ends_with(suffix);
    }

    // No wildcards, exact match
    pattern == name
}

#[cfg(test)]
mod tests {
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
}
