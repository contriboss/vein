use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use tokio::runtime::Runtime;
use vein::{config::Config, quarantine};
use vein_adapter::{CacheBackend, CacheBackendTrait, VersionStatus};

use super::setup::{build_current_thread_runtime, connect_cache_index, load_config};

struct QuarantineContext {
    config: Arc<Config>,
    rt: Runtime,
    index: Arc<CacheBackend>,
}

pub(crate) fn run_quarantine_status(config_path: PathBuf) -> Result<()> {
    let Some(ctx) = load_quarantine_context(config_path)? else {
        println!("Quarantine feature is disabled in configuration.");
        println!("Enable it by setting delay_policy.enabled = true in vein.toml");
        return Ok(());
    };

    let stats = ctx
        .rt
        .block_on(ctx.index.quarantine_stats())
        .context("fetching quarantine stats")?;

    println!("Quarantine Status");
    println!("=================");
    println!("Quarantined:     {}", stats.total_quarantined);
    println!("Available:       {}", stats.total_available);
    println!("Pinned:          {}", stats.total_pinned);
    println!("Blocked/Yanked:  {}", stats.total_yanked);
    println!();
    println!("Releasing today:      {}", stats.versions_releasing_today);
    println!(
        "Releasing this week:  {}",
        stats.versions_releasing_this_week
    );
    println!();
    println!(
        "Default delay: {} days",
        ctx.config.delay_policy.default_delay_days
    );
    println!("Skip weekends: {}", ctx.config.delay_policy.skip_weekends);

    Ok(())
}

pub(crate) fn run_quarantine_list(config_path: PathBuf, limit: u32) -> Result<()> {
    let Some(ctx) = load_quarantine_context(config_path)? else {
        println!("Quarantine feature is disabled in configuration.");
        return Ok(());
    };

    let versions = ctx
        .rt
        .block_on(ctx.index.get_all_quarantined(limit, 0))
        .context("fetching quarantined versions")?;

    if versions.is_empty() {
        println!("No versions currently in quarantine.");
        return Ok(());
    }

    println!(
        "{:<30} {:<15} {:<15} {:<10} AVAILABLE AFTER",
        "GEM", "VERSION", "PLATFORM", "STATUS"
    );
    println!("{}", "-".repeat(90));

    for version in versions {
        let platform = version.platform.as_deref().unwrap_or("ruby");
        println!(
            "{:<30} {:<15} {:<15} {:<10} {}",
            version.name,
            version.version,
            platform,
            format!("{:?}", version.status),
            version.available_after.format("%Y-%m-%d %H:%M UTC")
        );
    }

    Ok(())
}

pub(crate) fn run_quarantine_promote(config_path: PathBuf) -> Result<()> {
    let Some(ctx) = load_quarantine_context(config_path)? else {
        println!("Quarantine feature is disabled in configuration.");
        return Ok(());
    };

    let count = ctx
        .rt
        .block_on(quarantine::promote_now(ctx.index.as_ref()))
        .context("promoting expired quarantines")?;

    if count > 0 {
        println!(
            "Promoted {} version(s) from quarantine to available.",
            count
        );
    } else {
        println!("No versions ready for promotion.");
    }

    Ok(())
}

pub(crate) fn run_quarantine_approve(
    config_path: PathBuf,
    gem: String,
    version: String,
    platform: Option<String>,
    reason: String,
) -> Result<()> {
    let Some(ctx) = load_quarantine_context(config_path)? else {
        println!("Quarantine feature is disabled in configuration.");
        return Ok(());
    };

    ctx.rt
        .block_on(ctx.index.update_version_status(
            &gem,
            &version,
            platform.as_deref(),
            VersionStatus::Pinned,
            Some(format!("approved: {}", reason)),
        ))
        .context("approving version")?;

    let platform_str = platform.as_deref().unwrap_or("ruby");
    println!(
        "Approved {}-{} ({}) for immediate availability.",
        gem, version, platform_str
    );
    println!("Reason: {}", reason);

    Ok(())
}

pub(crate) fn run_quarantine_block(
    config_path: PathBuf,
    gem: String,
    version: String,
    platform: Option<String>,
    reason: String,
) -> Result<()> {
    let Some(ctx) = load_quarantine_context(config_path)? else {
        println!("Quarantine feature is disabled in configuration.");
        return Ok(());
    };

    ctx.rt
        .block_on(ctx.index.update_version_status(
            &gem,
            &version,
            platform.as_deref(),
            VersionStatus::Yanked,
            Some(format!("blocked: {}", reason)),
        ))
        .context("blocking version")?;

    let platform_str = platform.as_deref().unwrap_or("ruby");
    println!("Blocked {}-{} ({}).", gem, version, platform_str);
    println!("Reason: {}", reason);

    Ok(())
}

fn load_quarantine_context(config_path: PathBuf) -> Result<Option<QuarantineContext>> {
    let config = load_config(config_path)?;
    if !config.delay_policy.enabled {
        return Ok(None);
    }

    let rt = build_current_thread_runtime("quarantine")?;
    let (index, _) = connect_cache_index(&rt, &config)?;

    Ok(Some(QuarantineContext { config, rt, index }))
}
