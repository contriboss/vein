use anyhow::{Context, Result};
use tracing::{error, info};
use vein::{config::Config as VeinConfig, gem_metadata::indexer};
use vein_adapter::{CacheBackend, CacheBackendTrait};

use crate::commands::AppContext;

pub async fn run(name: String, version: Option<String>) -> Result<()> {
    let ctx = AppContext::init().await?;

    // Run symbols migrations to ensure table exists
    ctx.cache
        .run_symbols_migrations()
        .await
        .context("running symbols migrations")?;

    if let Some(ver) = version {
        // Index specific version
        info!(gem = %name, version = %ver, "Indexing gem");
        index_gem_version(&ctx.cache, &ctx.config, &name, &ver).await?;
        info!(gem = %name, version = %ver, "✓ Indexed successfully");
    } else {
        // Get all cached versions for this gem
        info!(gem = %name, "Fetching cached versions");
        let versions = get_gem_versions(&ctx.cache, &name).await?;

        if versions.is_empty() {
            error!(gem = %name, "No cached versions found");
            return Err(anyhow::anyhow!("No cached versions found for gem '{}'", name));
        }

        info!(gem = %name, count = versions.len(), "Found cached versions");

        for ver in &versions {
            info!(gem = %name, version = %ver, "Indexing");
            match index_gem_version(&ctx.cache, &ctx.config, &name, ver).await {
                Ok(_) => info!(gem = %name, version = %ver, "✓ Indexed"),
                Err(e) => error!(gem = %name, version = %ver, error = %e, "✗ Failed to index"),
            }
        }

        info!(gem = %name, total = versions.len(), "✓ Indexing complete");
    }

    Ok(())
}

async fn index_gem_version(
    cache: &CacheBackend,
    config: &VeinConfig,
    name: &str,
    version: &str,
) -> Result<()> {
    // Build gem file path
    let gem_filename = format!("{}-{}.gem", name, version);
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

    info!(path = %gem_path.display(), "Parsing gem");

    // Clear existing symbols for this version
    cache
        .clear_symbols(name, version, None)
        .await
        .context("clearing existing symbols")?;

    // Index the gem
    let file_symbols = indexer::index_gem(&gem_path).context("indexing gem")?;

    let mut total_symbols = 0;
    for file_symbol in file_symbols {
        for symbol in file_symbol.symbols {
            cache
                .insert_symbols(
                    name,
                    version,
                    None, // platform
                    &file_symbol.file_path,
                    symbol.symbol_type.as_str(),
                    &symbol.name,
                    symbol.parent.as_deref(),
                    Some(symbol.line as i32),
                )
                .await
                .context("inserting symbol")?;

            total_symbols += 1;
        }
    }

    info!(
        gem = %name,
        version = %version,
        symbols = total_symbols,
        "Indexed symbols"
    );

    Ok(())
}

async fn get_gem_versions(cache: &CacheBackend, name: &str) -> Result<Vec<String>> {
    // Get all cached gems
    let all_gems = cache
        .get_all_gems()
        .await
        .context("fetching all gems")?;

    // Filter for this gem name
    let versions: Vec<String> = all_gems
        .into_iter()
        .filter_map(|(gem_name, version)| {
            if gem_name == name {
                Some(version)
            } else {
                None
            }
        })
        .collect();

    Ok(versions)
}
