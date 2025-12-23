//! Quarantine background tasks.
//!
//! Provides scheduled jobs for managing gem version quarantine:
//! - Automatic promotion of versions when quarantine expires
//! - Database migration for quarantine tables

use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use rama::telemetry::tracing;
use tokio_cron_scheduler::{Job, JobScheduler};
use vein_adapter::CacheBackend;

use crate::config::DelayPolicyConfig;

/// Default cron schedule for quarantine promotion (every hour at minute 5)
pub const DEFAULT_PROMOTION_SCHEDULE: &str = "0 5 * * * *";

/// Spawns the quarantine promotion scheduler.
///
/// This job runs periodically to promote versions whose quarantine period has expired.
pub fn spawn_promotion_scheduler(
    config: &DelayPolicyConfig,
    index: Arc<dyn CacheBackend>,
    schedule: Option<&str>,
) {
    if !config.enabled {
        tracing::info!("Quarantine not enabled, skipping promotion scheduler");
        return;
    }

    let schedule = schedule.unwrap_or(DEFAULT_PROMOTION_SCHEDULE).to_string();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create quarantine scheduler runtime");

        rt.block_on(async {
            let sched = JobScheduler::new()
                .await
                .expect("Failed to create quarantine scheduler");

            let job = Job::new_async(schedule.as_str(), move |_uuid, _l| {
                let index = index.clone();
                Box::pin(async move {
                    let now = Utc::now();
                    tracing::debug!("Running quarantine promotion check");

                    match index.promote_expired_quarantines(now).await {
                        Ok(count) if count > 0 => {
                            tracing::info!(
                                promoted = count,
                                "Promoted quarantined versions to available"
                            );
                        }
                        Ok(_) => {
                            tracing::debug!("No quarantined versions ready for promotion");
                        }
                        Err(err) => {
                            tracing::error!(error = %err, "Failed to promote quarantined versions");
                        }
                    }
                })
            })
            .expect("Failed to create promotion job");

            sched
                .add(job)
                .await
                .expect("Failed to add promotion job to scheduler");

            sched
                .start()
                .await
                .expect("Failed to start quarantine scheduler");

            tracing::info!(
                schedule = %schedule,
                "Quarantine promotion scheduler started"
            );

            // Keep the scheduler runtime alive forever
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
            }
        });
    });
}

/// Ensures quarantine database tables exist.
///
/// Should be called during startup before using quarantine features.
pub async fn ensure_tables(index: &dyn CacheBackend, config: &DelayPolicyConfig) -> Result<()> {
    if !config.enabled {
        return Ok(());
    }

    if !index
        .quarantine_table_exists()
        .await
        .context("checking quarantine table")?
    {
        tracing::info!("Creating quarantine database tables");
        index
            .run_quarantine_migrations()
            .await
            .context("running quarantine migrations")?;
        tracing::info!("Quarantine tables created");
    }

    Ok(())
}

/// Manually triggers promotion of expired quarantines.
///
/// Useful for CLI commands.
pub async fn promote_now(index: &dyn CacheBackend) -> Result<u64> {
    let now = Utc::now();
    let count = index
        .promote_expired_quarantines(now)
        .await
        .context("promoting expired quarantines")?;
    Ok(count)
}
