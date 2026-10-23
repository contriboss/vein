//! Delay policy configuration for supply chain protection.
//!
//! Configures the quarantine buffer that delays new versions of gems,
//! crates and npm packages from appearing in the index, protecting against
//! supply chain attacks.

use serde::Deserialize;
use vein_adapter::{AssetKind, DelayPolicy as AdapterDelayPolicy};

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
    /// Per-package delay overrides; the first match wins. `gems` is the
    /// name configs used when only gems were delayed.
    #[serde(default, alias = "gems")]
    pub packages: Vec<PackageDelayOverride>,
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

    /// The timing for package `name` in registry `kind`: the defaults with
    /// its override's delay, if one matches.
    #[must_use]
    pub fn policy_for(&self, kind: AssetKind, name: &str) -> AdapterDelayPolicy {
        AdapterDelayPolicy {
            default_delay_days: self.delay_days_for(kind, name),
            skip_weekends: self.skip_weekends,
            business_hours_only: self.business_hours_only,
            release_hour_utc: self.release_hour_utc,
        }
    }

    /// Delay in days for package `name` in registry `kind`.
    #[must_use]
    pub fn delay_days_for(&self, kind: AssetKind, name: &str) -> u32 {
        self.packages
            .iter()
            .find(|entry| covers(entry.registry, kind) && entry.matches(name))
            .map_or(self.default_delay_days, |entry| entry.delay_days)
    }

    /// Why `name` `version` in registry `kind` is pinned, if it is.
    #[must_use]
    pub fn pin_reason(&self, kind: AssetKind, name: &str, version: &str) -> Option<&str> {
        self.pinned
            .iter()
            .find(|pin| covers(pin.registry, kind) && pin.name == name && pin.version == version)
            .map(|pin| pin.reason.as_str())
    }
}

/// An entry without a registry covers that name in every registry.
fn covers(registry: Option<AssetKind>, kind: AssetKind) -> bool {
    registry.is_none_or(|registry| registry == kind)
}

impl Default for DelayPolicyConfig {
    fn default() -> Self {
        Self {
            enabled: false, // Opt-in by default
            default_delay_days: Self::default_delay_days(),
            skip_weekends: Self::default_skip_weekends(),
            business_hours_only: Self::default_business_hours_only(),
            release_hour_utc: Self::default_release_hour_utc(),
            packages: Vec::new(),
            pinned: Vec::new(),
        }
    }
}

/// A delay for one package, or a glob of them.
#[derive(Debug, Clone, Deserialize)]
pub struct PackageDelayOverride {
    /// `gem`, `crate` or `npm`; every registry when left out.
    #[serde(default)]
    pub registry: Option<AssetKind>,
    /// Package name (or glob pattern if `pattern` is true).
    #[serde(default)]
    pub name: String,
    /// More exact names under the same delay: a publisher's own packages,
    /// one entry for all of them.
    #[serde(default)]
    pub names: Vec<String>,
    /// Delay in days for this package; 0 releases it at once.
    pub delay_days: u32,
    /// If true, `name` is a glob pattern (e.g., "rails-*", "@my-org/*").
    #[serde(default)]
    pub pattern: bool,
}

impl PackageDelayOverride {
    fn matches(&self, name: &str) -> bool {
        let named = if self.pattern {
            glob_match(&self.name, name)
        } else {
            !self.name.is_empty() && self.name == name
        };
        named || self.names.iter().any(|listed| listed == name)
    }
}

/// Pinned version configuration (bypass quarantine).
#[derive(Debug, Clone, Deserialize)]
pub struct PinnedVersion {
    /// `gem`, `crate` or `npm`; every registry when left out.
    #[serde(default)]
    pub registry: Option<AssetKind>,
    /// Package name.
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
