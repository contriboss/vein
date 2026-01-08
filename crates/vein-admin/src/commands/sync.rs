use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use rama::{http::{Uri, body::util::BodyExt, header::HeaderMap}, tls::rustls::dep::rustls};
use sha2::{Digest, Sha256};
use tracing::{error, info, warn};
use vein::{config::Config as VeinConfig, db, gem_metadata::extract_gem_metadata, upstream::UpstreamClient};
use vein_adapter::{AssetKey, AssetKind, CacheBackend, CacheBackendTrait, FilesystemStorage};

pub async fn run(
    name: String,
    version: Option<String>,
    platform: Option<String>,
) -> Result<()> {
    // Initialize rustls crypto provider (required for TLS)
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .map_err(|e| anyhow!("Failed to install rustls crypto provider: {:?}", e))?;

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

    // Check if upstream is configured
    let upstream_config = config
        .upstream
        .as_ref()
        .ok_or_else(|| anyhow!("No upstream configured - cannot sync"))?;

    info!(upstream = %upstream_config.url, "Connecting to upstream");

    // Create upstream client
    let client = UpstreamClient::new(upstream_config)
        .context("creating upstream client")?;

    // Initialize storage
    let storage = Arc::new(FilesystemStorage::new(config.storage.path.clone()));

    if let Some(ver) = version {
        // Sync specific version
        info!(gem = %name, version = %ver, platform = ?platform, "Syncing gem");
        sync_gem(
            &client,
            &cache,
            &storage,
            upstream_config.url.clone(),
            &name,
            &ver,
            platform.as_deref(),
        )
        .await?;
        info!(gem = %name, version = %ver, "✓ Synced successfully");
    } else {
        // Fetch info for gem to get all versions
        info!(gem = %name, "Fetching versions");
        let versions = fetch_gem_versions(&client, upstream_config.url.clone(), &name).await?;

        if versions.is_empty() {
            error!(gem = %name, "No versions found");
            return Err(anyhow!("No versions found for gem '{}'", name));
        }

        info!(gem = %name, count = versions.len(), "Found versions");

        for ver in &versions {
            info!(gem = %name, version = %ver, "Syncing");
            match sync_gem(
                &client,
                &cache,
                &storage,
                upstream_config.url.clone(),
                &name,
                ver,
                platform.as_deref(),
            )
            .await
            {
                Ok(_) => info!(gem = %name, version = %ver, "✓ Synced"),
                Err(e) => error!(gem = %name, version = %ver, error = %e, "✗ Failed to sync"),
            }
        }

        info!(gem = %name, total = versions.len(), "✓ Sync complete");
    }

    Ok(())
}

async fn sync_gem(
    client: &UpstreamClient,
    cache: &CacheBackend,
    storage: &FilesystemStorage,
    base_url: Uri,
    name: &str,
    version: &str,
    platform: Option<&str>,
) -> Result<()> {
    // Build gem filename
    let file_name = if let Some(plat) = platform {
        format!("{}-{}-{}.gem", name, version, plat)
    } else {
        format!("{}-{}.gem", name, version)
    };

    // Check if already cached
    let asset_key = AssetKey {
        kind: AssetKind::Gem,
        name,
        version,
        platform,
    };

    if cache.get(&asset_key).await?.is_some() {
        info!(gem = %name, version = %version, "Already cached, skipping");
        return Ok(());
    }

    // Build upstream URL
    let gem_path = format!("/gems/{}", file_name);
    let url = build_url(&base_url, &gem_path)?;

    info!(url = %url, "Fetching from upstream");

    // Fetch from upstream
    let headers = HeaderMap::new();
    let response = client
        .get_with_headers(url, &headers)
        .await
        .context("fetching gem from upstream")?;

    let status = response.status();
    if !status.is_success() {
        return Err(anyhow!(
            "upstream returned {}: {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or("unknown")
        ));
    }

    // Read body
    let body_bytes = response
        .into_body()
        .collect()
        .await
        .context("reading response body")?
        .to_bytes();

    // Compute checksum
    let sha_hex = {
        let mut hasher = Sha256::new();
        hasher.update(&body_bytes);
        hex::encode(hasher.finalize())
    };

    // Determine storage path
    let relative_path = format!("gems/{}/{}", name, file_name);

    // Ensure parent directory exists
    let final_path = storage.resolve(&relative_path);
    if let Some(parent) = final_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .context("creating parent directories")?;
    }

    // Write to disk
    tokio::fs::write(&final_path, &body_bytes)
        .await
        .context("writing gem file")?;

    // Store in cache index
    cache
        .insert_or_replace(
            &asset_key,
            &relative_path,
            &sha_hex,
            body_bytes.len() as u64,
        )
        .await
        .context("storing in cache index")?;

    info!(
        gem = %name,
        version = %version,
        size = body_bytes.len(),
        sha256 = &sha_hex[..8],
        "Cached successfully"
    );

    // Extract and store metadata
    match extract_gem_metadata(
        &final_path,
        name,
        version,
        platform,
        body_bytes.len() as u64,
        &sha_hex,
        None,
    )
    .await
    {
        Ok(Some(metadata)) => {
            if let Err(e) = cache.upsert_metadata(&metadata).await {
                warn!(gem = %name, version = %version, error = %e, "Failed to store metadata");
            } else {
                info!(gem = %name, version = %version, "Metadata extracted and stored");
            }
        }
        Ok(None) => {
            warn!(gem = %name, version = %version, "No metadata found in gem");
        }
        Err(e) => {
            warn!(gem = %name, version = %version, error = %e, "Failed to extract metadata");
        }
    }

    Ok(())
}

async fn fetch_gem_versions(
    client: &UpstreamClient,
    base_url: Uri,
    name: &str,
) -> Result<Vec<String>> {
    // Fetch compact index info for the gem
    let info_path = format!("/info/{}", name);
    let url = build_url(&base_url, &info_path)?;

    let headers = HeaderMap::new();
    let response = client
        .get_with_headers(url, &headers)
        .await
        .context("fetching gem info from upstream")?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "upstream returned {}: {}",
            response.status().as_u16(),
            response
                .status()
                .canonical_reason()
                .unwrap_or("unknown")
        ));
    }

    let body_bytes = response
        .into_body()
        .collect()
        .await
        .context("reading info response")?
        .to_bytes();

    let content = String::from_utf8(body_bytes.to_vec())
        .context("info response is not valid UTF-8")?;

    // Parse compact index format
    // Format: version platform|platform checksum,...
    // Example: 1.0.0 ruby abc123,...
    let mut versions = Vec::new();
    for line in content.lines() {
        if let Some(version) = line.split_whitespace().next().filter(|v| !v.is_empty() && *v != "---") {
            versions.push(version.to_string());
        }
    }

    Ok(versions)
}

fn build_url(base: &Uri, path: &str) -> Result<Uri> {
    let base_path = base
        .path_and_query()
        .map(|pq| pq.path())
        .unwrap_or("/")
        .trim_end_matches('/');

    let combined = if base_path.is_empty() || base_path == "/" {
        path.to_string()
    } else {
        format!("{}{}", base_path, path)
    };

    let mut parts = base.clone().into_parts();
    parts.path_and_query = Some(combined.parse().context("parsing combined path")?);

    Uri::from_parts(parts).context("building URL")
}
