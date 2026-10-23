//! Quarantine background tasks: promoting versions whose quarantine has
//! expired, on a cron schedule evaluated in UTC.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use croner::Cron;
use jiff::Timestamp;
use jiff::tz::TimeZone;
use rama::telemetry::tracing;
use vein_adapter::{CacheBackend, CacheBackendTrait};

use crate::config::DelayPolicyConfig;

/// Default cron schedule for quarantine promotion: every hour at minute 5
/// (`sec min hour day month weekday`).
pub const DEFAULT_PROMOTION_SCHEDULE: &str = "0 5 * * * *";

/// The promotion loop, for the server runtime to spawn; `None` when the
/// delay policy is off. The schedule is parsed here, so a bad one stops
/// startup instead of the scheduler.
pub fn promotion_task(
    config: &DelayPolicyConfig,
    index: Arc<CacheBackend>,
    schedule: Option<&str>,
) -> Result<Option<impl Future<Output = ()> + Send + 'static>> {
    if !config.enabled {
        tracing::info!("Quarantine not enabled, skipping promotion scheduler");
        return Ok(None);
    }

    let cron = parse_schedule(schedule.unwrap_or(DEFAULT_PROMOTION_SCHEDULE))?;
    tracing::info!(schedule = %cron, "Quarantine promotion scheduler started");

    Ok(Some(async move {
        loop {
            let Some(wait) = until_next(&cron, Timestamp::now()) else {
                tracing::error!(%cron, "schedule has no next run; promotion scheduler stopped");
                return;
            };
            tokio::time::sleep(wait).await;
            promote_logged(&index).await;
        }
    }))
}

fn parse_schedule(pattern: &str) -> Result<Cron> {
    pattern
        .parse()
        .with_context(|| format!("parsing the quarantine promotion schedule `{pattern}`"))
}

/// How long from `now` until `cron` next fires; `None` when it never does.
fn until_next(cron: &Cron, now: Timestamp) -> Option<Duration> {
    let now = now.to_zoned(TimeZone::UTC);
    let next = cron.find_next_occurrence(&now, false).ok()?;
    // `next` is strictly after `now`, so the zero fallback is never taken.
    Some(Duration::try_from(next.duration_since(&now)).unwrap_or(Duration::ZERO))
}

async fn promote_logged(index: &CacheBackend) {
    tracing::debug!("Running quarantine promotion check");
    match index.promote_expired_quarantines(Timestamp::now()).await {
        Ok(0) => tracing::debug!("No quarantined versions ready for promotion"),
        Ok(promoted) => {
            tracing::info!(promoted, "Promoted quarantined versions to available");
        }
        Err(err) => tracing::error!(error = %err, "Failed to promote quarantined versions"),
    }
}

/// Manually triggers promotion of expired quarantines.
///
/// Useful for CLI commands.
pub async fn promote_now(index: &CacheBackend) -> Result<u64> {
    index
        .promote_expired_quarantines(Timestamp::now())
        .await
        .context("promoting expired quarantines")
}

#[cfg(test)]
mod tests;
