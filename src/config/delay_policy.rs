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
    /// Hour in UTC for `business_hours_only` releases (0-23).
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
    const fn default_delay_days() -> u32 {
        3
    }

    const fn default_skip_weekends() -> bool {
        true
    }

    const fn default_business_hours_only() -> bool {
        true
    }

    const fn default_release_hour_utc() -> u8 {
        9
    }

    /// Convert to the adapter's `DelayPolicy` type.
    #[must_use]
    pub const fn to_adapter_policy(&self) -> AdapterDelayPolicy {
        AdapterDelayPolicy {
            default_delay_days: self.default_delay_days,
            skip_weekends: self.skip_weekends,
            business_hours_only: self.business_hours_only,
            release_hour_utc: self.release_hour_utc,
        }
    }

    /// Get the delay days for a specific gem, considering overrides.
    #[must_use]
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
    #[must_use]
    pub fn is_pinned(&self, name: &str, version: &str) -> bool {
        self.pinned
            .iter()
            .any(|p| p.name == name && p.version == version)
    }

    /// Get the pin reason if version is pinned.
    #[must_use]
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
mod tests;
