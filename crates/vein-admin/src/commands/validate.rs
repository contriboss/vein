use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::{error, info};
use vein::{config::Config as VeinConfig, db, gem_metadata::validate_binary_architectures};
use vein_adapter::CacheBackendKind;

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
        validate_gem_version(&cache, &config, &name, &ver).await?;
    } else {
        // Get all cached versions for this gem
        info!(gem = %name, "Fetching cached versions");
        let versions = get_gem_versions(&cache, &name).await?;

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
    cache: &CacheBackendKind,
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
    let (actual_version, claimed_platform): (String, Option<String>) = if platform.is_some() {
        (version.to_string(), platform.map(|s| s.to_string()))
    } else {
        // Try to extract platform from version string
        // E.g., "0.0.52-arm64-darwin" -> ("0.0.52", Some("arm64-darwin"))
        // Split by '-' to separate version from platform
        let parts: Vec<&str> = version.splitn(2, '-').collect();
        if parts.len() == 2 {
            // First part is version (e.g., "0.0.52"), second is platform (e.g., "arm64-darwin")
            (parts[0].to_string(), Some(parts[1].to_string()))
        } else {
            (version.to_string(), None)
        }
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

async fn get_gem_versions(cache: &CacheBackendKind, name: &str) -> Result<Vec<(String, Option<String>)>> {
    // Get all cached gems
    let all_gems = cache
        .get_all_gems()
        .await
        .context("fetching all gems")?;

    // Filter for this gem name and collect (version, platform) tuples
    let versions: Vec<(String, Option<String>)> = all_gems
        .into_iter()
        .filter_map(|(gem_name, version_platform)| {
            if gem_name == name {
                Some((version_platform, None))
            } else {
                None
            }
        })
        .collect();

    Ok(versions)
}
