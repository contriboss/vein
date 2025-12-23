//! Quarantine filtering for compact index responses.
//!
//! Filters quarantined versions from compact index responses to prevent
//! `bundle update` and `bundle outdated` from seeing versions still in quarantine.

use anyhow::Result;
use chrono::Utc;
use rama::telemetry::tracing::{debug, warn};
use vein_adapter::{
    CacheBackend, GemVersion, VersionStatus, calculate_availability, is_version_available,
};

use crate::config::DelayPolicyConfig;

/// Records a new gem version in the quarantine system.
///
/// Called when a gem is fetched from upstream for the first time.
pub async fn record_new_version(
    config: &DelayPolicyConfig,
    index: &dyn CacheBackend,
    name: &str,
    version: &str,
    platform: Option<&str>,
    sha256: &str,
) -> Result<()> {
    if !config.enabled {
        return Ok(());
    }

    // Check if we already have this version recorded
    if let Ok(Some(_existing)) = index.get_gem_version(name, version, platform).await {
        // Already recorded, don't overwrite
        return Ok(());
    }

    let delay_days = config.delay_for_gem(name);
    let now = Utc::now();
    let available_after = calculate_availability(now, &config.to_adapter_policy());

    let status = if delay_days == 0 || config.is_pinned(name, version) {
        VersionStatus::Pinned
    } else {
        VersionStatus::Quarantine
    };

    let status_reason = if status == VersionStatus::Pinned {
        config
            .pin_reason(name, version)
            .map(|r| format!("pinned: {}", r))
            .or_else(|| Some("zero-delay gem".to_string()))
    } else {
        Some("auto".to_string())
    };

    let gem_version = GemVersion {
        id: 0,
        name: name.to_string(),
        version: version.to_string(),
        platform: platform.map(String::from),
        sha256: Some(sha256.to_string()),
        published_at: now,
        available_after,
        status,
        status_reason,
        upstream_yanked: false,
        created_at: now,
        updated_at: now,
    };

    index.upsert_gem_version(&gem_version).await?;

    debug!(
        gem = %name,
        version = %version,
        status = %status,
        available_after = %available_after,
        "Recorded new gem version in quarantine system"
    );

    Ok(())
}

/// Filters quarantined versions from a compact index `/info/{gem}` response.
///
/// The compact index format is:
/// ```text
/// ---
/// 1.0.0 |checksum|dep1,dep2
/// 1.1.0 platform|checksum|dep1
/// ```
///
/// Returns the filtered response body.
pub async fn filter_compact_info(
    config: &DelayPolicyConfig,
    index: &dyn CacheBackend,
    gem_name: &str,
    body: &[u8],
) -> Result<Vec<u8>> {
    if !config.enabled {
        return Ok(body.to_vec());
    }

    let body_str = match std::str::from_utf8(body) {
        Ok(s) => s,
        Err(_) => return Ok(body.to_vec()), // Not UTF-8, pass through
    };

    let now = Utc::now();

    // Get quarantine status for all versions of this gem
    let versions = match index.get_gem_versions_for_index(gem_name).await {
        Ok(v) => v,
        Err(err) => {
            warn!(
                error = %err,
                gem = %gem_name,
                "Failed to fetch quarantine status, passing through unfiltered"
            );
            return Ok(body.to_vec());
        }
    };

    // If no versions tracked yet, pass through (quarantine system not populated for this gem)
    if versions.is_empty() {
        return Ok(body.to_vec());
    }

    // Build a set of quarantined versions
    let quarantined: std::collections::HashSet<String> = versions
        .iter()
        .filter(|v| !is_version_available(v, now))
        .map(|v| format_version_key(&v.version, v.platform.as_deref()))
        .collect();

    if quarantined.is_empty() {
        return Ok(body.to_vec()); // Nothing to filter
    }

    // Filter the compact index lines
    let mut output_lines = Vec::new();
    for line in body_str.lines() {
        if line == "---" || line.is_empty() {
            output_lines.push(line);
            continue;
        }

        // Parse the line to extract version and platform
        // Format: "version platform|checksum|deps" or "version |checksum|deps"
        if let Some((version_key, _rest)) = parse_compact_line(line)
            && quarantined.contains(&version_key)
        {
            debug!(
                gem = %gem_name,
                version_key = %version_key,
                "Filtering quarantined version from compact index"
            );
            continue; // Skip this line
        }

        output_lines.push(line);
    }

    Ok(output_lines.join("\n").into_bytes())
}

/// Parses a compact index line to extract the version key.
///
/// Returns (version_key, rest_of_line) where version_key is "version" or "version:platform"
fn parse_compact_line(line: &str) -> Option<(String, &str)> {
    // Find the first pipe which separates version info from checksum
    let pipe_pos = line.find('|')?;
    let version_part = line[..pipe_pos].trim();
    let rest = &line[pipe_pos..];

    // version_part is either "1.0.0" or "1.0.0 x86_64-linux"
    let mut parts = version_part.split_whitespace();
    let version = parts.next()?;
    let platform = parts.next();

    let key = format_version_key(version, platform);
    Some((key, rest))
}

/// Formats a version key for lookup.
fn format_version_key(version: &str, platform: Option<&str>) -> String {
    match platform {
        Some(p) if p != "ruby" => format!("{}:{}", version, p),
        _ => version.to_string(),
    }
}

/// Filters quarantined versions from a `/versions` response.
///
/// The versions format is:
/// ```text
/// created_at: 2026-01-01T00:00:00Z
/// ---
/// gem_name 1.0.0,1.1.0,1.2.0 checksum
/// other_gem 2.0.0 checksum
/// ```
///
/// This is more complex because we'd need to filter per-gem.
/// For now, we pass through unfiltered - the `/info/{gem}` filtering is sufficient.
#[allow(dead_code, clippy::unused_async)]
pub async fn filter_compact_versions(
    config: &DelayPolicyConfig,
    _index: &dyn CacheBackend,
    body: &[u8],
) -> Result<Vec<u8>> {
    if !config.enabled {
        return Ok(body.to_vec());
    }

    // TODO: Implement versions filtering if needed.
    // The /info/{gem} filtering should be sufficient for most use cases
    // since bundle/bundler will still query /info/{gem} for the actual versions.
    Ok(body.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_compact_line_simple() {
        let (key, rest) = parse_compact_line("1.0.0 |abc123|dep1").unwrap();
        assert_eq!(key, "1.0.0");
        assert_eq!(rest, "|abc123|dep1");
    }

    #[test]
    fn test_parse_compact_line_with_platform() {
        let (key, rest) = parse_compact_line("1.0.0 x86_64-linux|abc123|dep1").unwrap();
        assert_eq!(key, "1.0.0:x86_64-linux");
        assert_eq!(rest, "|abc123|dep1");
    }

    #[test]
    fn test_parse_compact_line_ruby_platform() {
        let (key, _) = parse_compact_line("1.0.0 ruby|abc123|").unwrap();
        assert_eq!(key, "1.0.0"); // ruby platform treated as no platform
    }

    #[test]
    fn test_format_version_key() {
        assert_eq!(format_version_key("1.0.0", None), "1.0.0");
        assert_eq!(format_version_key("1.0.0", Some("ruby")), "1.0.0");
        assert_eq!(
            format_version_key("1.0.0", Some("x86_64-linux")),
            "1.0.0:x86_64-linux"
        );
    }
}
