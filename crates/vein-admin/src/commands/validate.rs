use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::{error, info};
use vein::{config::Config as VeinConfig, db, gem_metadata::validate_binary_architectures};
use vein_adapter::{CacheBackend, CacheBackendTrait};

/// Checks if a string looks like a valid RubyGems platform identifier.
///
/// This helps distinguish between version suffixes (e.g., "1.0.0-pre1") and
/// actual platform identifiers (e.g., "1.0.0-x86_64-linux").
fn looks_like_rubygems_platform(s: &str) -> bool {
    // Common RubyGems platform identifiers
    const KNOWN_PLATFORMS: &[&str] = &[
        "ruby",
        "jruby",
        "java",
        "x86-mswin32",
        "x64-mswin64",
        "x86-mingw32",
        "x64-mingw32",
        "x64-mingw-ucrt",
        "x86_64-linux",
        "x86-linux",
        "x86_64-darwin",
        "arm64-darwin",
        "universal-darwin",
        "x86_64-freebsd",
        "x86_64-openbsd",
        "x86_64-solaris",
    ];

    if KNOWN_PLATFORMS.contains(&s) {
        return true;
    }

    // Fallback: accept simple CPU-OS[-version] style strings with safe characters
    fn valid_segment(seg: &str) -> bool {
        !seg.is_empty()
            && seg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
    }

    let mut parts = s.split('-');
    let first = match parts.next() {
        Some(p) => p,
        None => return false,
    };
    let second = match parts.next() {
        Some(p) => p,
        None => return false,
    };
    let third = parts.next();

    // If there are more than three segments, it's unlikely to be a standard platform string.
    if parts.next().is_some() {
        return false;
    }

    if !valid_segment(first) || !valid_segment(second) {
        return false;
    }
    if let Some(third) = third {
        if !valid_segment(third) {
            return false;
        }
    }

    true
}

pub async fn run(name: String, version: Option<String>) -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .init();

    info!("Loading Vein configuration...");

    // Load vein config
    let config_path = std::env::var("VEIN_CONFIG_PATH")
        .ok()
        .map(std::path::PathBuf::from);
    let config = Arc::new(VeinConfig::load(config_path).context("loading config")?);

    // Initialize cache backend
    let (cache, _backend) = db::connect_cache_backend(&config)
        .await
        .context("connecting to cache backend")?;

    if let Some(ver) = version {
        // Validate specific version
        info!(gem = %name, version = %ver, "Validating gem");
        validate_gem_version(&*cache, &config, &name, &ver).await?;
    } else {
        // Get all cached versions for this gem
        info!(gem = %name, "Fetching cached versions");
        let versions = get_gem_versions(&*cache, &name).await?;

        if versions.is_empty() {
            error!(gem = %name, "No cached versions found");
            return Err(anyhow::anyhow!("No cached versions found for gem '{}'", name));
        }

        info!(gem = %name, count = versions.len(), "Found cached versions");

        for (ver, platform) in &versions {
            info!(gem = %name, version = %ver, platform = ?platform, "Validating");
            match validate_gem_version_with_platform(&config, &name, ver, platform.as_deref()).await {
                Ok(true) => info!(gem = %name, version = %ver, platform = ?platform, "✓ Valid"),
                Ok(false) => error!(gem = %name, version = %ver, platform = ?platform, "✗ Invalid"),
                Err(e) => error!(gem = %name, version = %ver, platform = ?platform, error = %e, "✗ Failed to validate"),
            }
        }

        info!(gem = %name, total = versions.len(), "✓ Validation complete");
    }

    Ok(())
}

async fn validate_gem_version(
    cache: &CacheBackend,
    config: &VeinConfig,
    name: &str,
    version: &str,
) -> Result<()> {
    // Get all platforms for this version
    let versions = get_gem_versions(cache, name).await?;
    let platforms: Vec<_> = versions
        .iter()
        .filter_map(|(v, p)| if v == version { Some(p.clone()) } else { None })
        .collect();

    if platforms.is_empty() {
        return Err(anyhow::anyhow!(
            "No cached gem found for {}-{}",
            name,
            version
        ));
    }

    let mut all_valid = true;
    for platform in platforms {
        let is_valid = validate_gem_version_with_platform(config, name, version, platform.as_deref()).await?;
        if !is_valid {
            all_valid = false;
        }
    }

    if !all_valid {
        return Err(anyhow::anyhow!("Validation failed for {}-{}", name, version));
    }

    Ok(())
}

async fn validate_gem_version_with_platform(
    config: &VeinConfig,
    name: &str,
    version: &str,
    platform: Option<&str>,
) -> Result<bool> {
    // Parse version which might contain platform (e.g., "0.0.52-arm64-darwin")
    // Only treat a suffix as platform if it looks like a valid RubyGems platform.
    let (actual_version, claimed_platform): (String, Option<String>) = if let Some(p) = platform {
        (version.to_string(), Some(p.to_string()))
    } else if let Some(idx) = version.find('-') {
        // Split into potential version and platform components.
        let (maybe_version, rest) = version.split_at(idx);
        // `rest` starts with '-', so skip it to get the candidate platform.
        let maybe_platform = &rest[1..];

        if looks_like_rubygems_platform(maybe_platform) {
            (maybe_version.to_string(), Some(maybe_platform.to_string()))
        } else {
            // Hyphen is part of the version (e.g., "1.0.0-pre1"), not a platform separator.
            (version.to_string(), None)
        }
    } else {
        (version.to_string(), None)
    };

    // Build gem file path
    let gem_filename = if let Some(plat) = claimed_platform.as_deref() {
        format!("{}-{}-{}.gem", name, actual_version, plat)
    } else {
        format!("{}-{}.gem", name, actual_version)
    };

    let gem_path = config
        .storage
        .path
        .join("gems")
        .join(name)
        .join(&gem_filename);

    if !gem_path.exists() {
        return Err(anyhow::anyhow!(
            "Gem file not found: {}",
            gem_path.display()
        ));
    }

    info!(path = %gem_path.display(), "Validating gem");

    // Validate binary architectures
    let validation = validate_binary_architectures(&gem_path, claimed_platform.as_deref())?;

    if validation.detected_binaries.is_empty() {
        info!(
            gem = %name,
            version = %version,
            platform = ?platform,
            "No native binaries found"
        );
        return Ok(true);
    }

    info!(
        gem = %name,
        version = %version,
        platform = ?platform,
        binaries = validation.detected_binaries.len(),
        valid = validation.is_valid,
        "Validation result"
    );

    if !validation.is_valid {
        error!("Architecture mismatches found:");
        for mismatch in &validation.mismatches {
            if let Some((_, binary_info)) = validation
                .detected_binaries
                .iter()
                .find(|(path, _)| path == mismatch)
            {
                error!(
                    "  ✗ {}\n    Claimed: {:?}\n    Detected: {}",
                    mismatch,
                    validation.claimed_platform,
                    binary_info.platform_string()
                );
            }
        }
        return Ok(false);
    }

    Ok(true)
}

async fn get_gem_versions(cache: &CacheBackend, name: &str) -> Result<Vec<(String, Option<String>)>> {
    // Get all versions for this gem (includes platform information)
    let gem_versions = cache
        .get_gem_versions_for_index(name)
        .await
        .context("fetching gem versions")?;

    // Extract (version, platform) tuples
    let versions: Vec<(String, Option<String>)> = gem_versions
        .into_iter()
        .map(|gv| (gv.version, gv.platform))
        .collect();

    Ok(versions)
}
