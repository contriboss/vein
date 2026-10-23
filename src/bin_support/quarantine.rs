use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use tokio::runtime::Runtime;
use vein::{config::Config, delay};
use vein_adapter::{AssetKey, CacheBackend, CacheBackendTrait, VersionStatus};

use super::setup::{build_current_thread_runtime, connect_cache_index, load_config};

struct QuarantineContext {
    config: Arc<Config>,
    rt: Runtime,
    index: Arc<CacheBackend>,
}

pub fn run_quarantine_status(config_path: PathBuf) -> Result<()> {
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
    println!("Blocked:         {}", stats.total_blocked);
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

pub fn run_quarantine_list(config_path: PathBuf, limit: u32) -> Result<()> {
    let Some(ctx) = load_quarantine_context(config_path)? else {
        println!("Quarantine feature is disabled in configuration.");
        return Ok(());
    };

    let versions = ctx
        .rt
        .block_on(ctx.index.held_versions(limit, 0))
        .context("fetching quarantined versions")?;

    if versions.is_empty() {
        println!("No versions currently in quarantine.");
        return Ok(());
    }

    println!(
        "{:<8} {:<30} {:<15} {:<15} {:<10} AVAILABLE AFTER",
        "REGISTRY", "NAME", "VERSION", "PLATFORM", "STATUS"
    );
    println!("{}", "-".repeat(99));

    for version in versions {
        println!(
            "{:<8} {:<30} {:<15} {:<15} {:<10} {}",
            version.kind.as_str(),
            version.name,
            version.version,
            version.platform.as_deref().unwrap_or("-"),
            version.status,
            version.available_after.strftime("%Y-%m-%d %H:%M UTC")
        );
    }

    Ok(())
}

pub fn run_quarantine_promote(config_path: PathBuf) -> Result<()> {
    let Some(ctx) = load_quarantine_context(config_path)? else {
        println!("Quarantine feature is disabled in configuration.");
        return Ok(());
    };

    let count = ctx
        .rt
        .block_on(delay::promote_now(ctx.index.as_ref()))
        .context("promoting expired quarantines")?;

    if count > 0 {
        println!("Promoted {count} version(s) from quarantine to available.");
    } else {
        println!("No versions ready for promotion.");
    }

    Ok(())
}

pub fn run_quarantine_approve(
    config_path: PathBuf,
    key: &AssetKey<'_>,
    reason: &str,
) -> Result<()> {
    decide(
        config_path,
        key,
        VersionStatus::Pinned,
        format!("approved: {reason}"),
    )?;
    println!("Approved {} for immediate availability.", describe(key));
    println!("Reason: {reason}");
    Ok(())
}

pub fn run_quarantine_block(config_path: PathBuf, key: &AssetKey<'_>, reason: &str) -> Result<()> {
    decide(
        config_path,
        key,
        VersionStatus::Blocked,
        format!("blocked: {reason}"),
    )?;
    println!(
        "Blocked {}: hidden from the index, downloads refused.",
        describe(key)
    );
    println!("Reason: {reason}");
    Ok(())
}

/// Applies a quarantine decision (status + audit note) to one version.
fn decide(
    config_path: PathBuf,
    key: &AssetKey<'_>,
    status: VersionStatus,
    note: String,
) -> Result<()> {
    let Some(ctx) = load_quarantine_context(config_path)? else {
        println!("Quarantine feature is disabled in configuration.");
        return Ok(());
    };
    ctx.rt
        .block_on(ctx.index.set_version_status(key, status, Some(note)))
        .with_context(|| format!("recording the decision on {}", describe(key)))
}

/// `gem rails 8.1.4 (x86_64-linux)`, `npm @types/node 26.6.5`.
fn describe(key: &AssetKey<'_>) -> String {
    let platform = key
        .platform
        .map(|platform| format!(" ({platform})"))
        .unwrap_or_default();
    format!(
        "{} {} {}{platform}",
        key.kind.as_str(),
        key.name,
        key.version
    )
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
