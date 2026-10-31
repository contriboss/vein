//! vein's background work as stampeded jobs, in vein's own Postgres: the
//! name catalogs' syncs and the release delay's promotions.
//!
//! Schedules are rows, so with several vein processes each occurrence runs
//! once, and one missed while vein was down runs once, late. Failed runs
//! retry with backoff; a run cut short (vein stopped mid-crawl) is reclaimed
//! and resumes from the catalog's checkpoint.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use rama::telemetry::tracing::info;
use sqlx::ConnectOptions;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use stampeded::{Maintenance, MaintenanceConfig, NewJob, Queue, Queues, Worker, WorkerConfig};
use tokio::sync::watch;
use vein_adapter::{AssetKind, CacheBackend};

use crate::config::Config;
use crate::{catalog, delay};

/// The stampeded queue vein's jobs run on.
pub const QUEUE: &str = "vein";
/// Every schedule vein owns starts so; others in the database are left be.
const KEY_PREFIX: &str = "vein:";
const PROMOTE: &str = "delay.promote";
/// crates.io's first crawl is an hour at its crawler limit; npm's walk, half.
const JOB_TIMEOUT: Duration = Duration::from_hours(4);

/// A recurring job vein keeps: cron expressions are UTC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recurring {
    pub key: String,
    pub task: &'static str,
    pub cron: &'static str,
}

/// The recurring jobs `config` asks for: each catalog every 6 hours (at
/// minute 17, off the hour everyone else picks), and the delay's promotion
/// hourly when the policy is on.
#[must_use]
pub fn recurring(config: &Config) -> Vec<Recurring> {
    let mut jobs: Vec<Recurring> = config
        .catalog
        .registries
        .iter()
        .map(|&kind| Recurring {
            key: catalog_key(kind),
            task: sync_task(kind),
            cron: "17 */6 * * *",
        })
        .collect();
    if config.delay_policy.enabled {
        jobs.push(Recurring {
            key: format!("{KEY_PREFIX}delay:promote"),
            task: PROMOTE,
            cron: "5 * * * *",
        });
    }
    jobs
}

/// Runs vein's jobs until `shutdown`: stampeded's schema and queue, the
/// schedules `config` asks for, a first sync of each catalog, a worker and
/// stampeded's maintenance (scheduler, reaper, pruner).
pub async fn run(
    config: Arc<Config>,
    index: Arc<CacheBackend>,
    shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let url = config.database.backend()?.url;
    let pool = stampeded::connect(PgPoolOptions::new().max_connections(4), &url)
        .await
        .context("connecting the job queue")?;
    stampeded::schema::migrate(&pool)
        .await
        .context("installing or upgrading stampeded's schema")?;
    let queue = Queue::new(pool);
    queue
        .create_queue(QUEUE)
        .await
        .context("creating vein's job queue")?;
    keep_schedules(&queue, &recurring(&config)).await?;

    // A new schedule first fires at its next occurrence; a catalog should not
    // wait 6 hours for its first sync. One per registry, however many starts.
    for &kind in &config.catalog.registries {
        queue
            .enqueue(NewJob::new(QUEUE, sync_task(kind)).unique(catalog_key(kind)))
            .await
            .context("enqueueing a catalog sync")?;
    }

    let mut worker = Worker::new(
        queue.clone(),
        WorkerConfig {
            queues: Queues::one(QUEUE),
            max_concurrency: 4,
            job_timeout: JOB_TIMEOUT,
            ..WorkerConfig::default()
        },
    );
    for kind in crate::registry::served() {
        let index = Arc::clone(&index);
        let url = url.clone();
        worker = worker.register(sync_task(kind), move |_job| {
            let index = Arc::clone(&index);
            let url = url.clone();
            async move { Ok(sync_exclusively(&url, &index, kind).await?) }
        });
    }
    let promote_index = Arc::clone(&index);
    worker = worker.register(PROMOTE, move |_job| {
        let index = Arc::clone(&promote_index);
        async move {
            let promoted = delay::promote_now(&index).await?;
            info!(promoted, "delay: promoted versions past their delay");
            Ok(())
        }
    });

    #[cfg(feature = "crates")]
    {
        worker = worker.register_job::<crate::crates::extraction_job::CrateExtractor>();
    }

    let maintenance = Maintenance::new(queue, MaintenanceConfig::default());
    tokio::try_join!(worker.run(shutdown.clone()), maintenance.run(shutdown))
        .context("running vein's jobs")?;
    Ok(())
}

/// Upserts `wanted`, and removes vein's schedules no longer wanted (a
/// registry dropped from the config, the delay turned off).
pub async fn keep_schedules(queue: &Queue, wanted: &[Recurring]) -> Result<()> {
    for job in wanted {
        queue
            .upsert_cron_schedule(&job.key, QUEUE, job.task, job.cron, "UTC", None)
            .await
            .with_context(|| format!("scheduling {}", job.key))?;
    }
    let keys: Vec<&str> = wanted.iter().map(|job| job.key.as_str()).collect();
    sqlx::query("DELETE FROM stampeded_schedules WHERE starts_with(key, $1) AND key <> ALL($2)")
        .bind(KEY_PREFIX)
        .bind(&keys)
        .execute(queue.pool())
        .await
        .context("removing schedules vein no longer keeps")?;
    Ok(())
}

/// Syncs registry `kind`'s catalog unless another vein process already is:
/// two walks would fight over one checkpoint. The lock lives on a connection
/// of its own, so it ends with the sync, crash included, never left behind
/// in a pool.
async fn sync_exclusively(url: &str, index: &CacheBackend, kind: AssetKind) -> Result<()> {
    let options: PgConnectOptions = url.parse().context("reading the database url")?;
    let mut lock = options
        .connect()
        .await
        .context("connecting for the catalog lock")?;
    let held: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
        .bind(lock_key(kind))
        .fetch_one(&mut lock)
        .await?;
    if !held {
        info!(
            registry = kind.as_str(),
            "catalog: another sync is running, standing down"
        );
        return Ok(());
    }
    if let Some(count) = catalog::sync_once(index, kind).await? {
        info!(registry = kind.as_str(), count, "catalog synced");
    } else {
        info!(registry = kind.as_str(), "catalog already up to date");
    }
    Ok(())
}

fn catalog_key(kind: AssetKind) -> String {
    format!("{KEY_PREFIX}catalog:{}", kind.as_str())
}

const fn sync_task(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Gem | AssetKind::Spec => "catalog.sync.gem",
        AssetKind::Crate => "catalog.sync.crate",
        AssetKind::NpmPackage => "catalog.sync.npm",
    }
}

/// `"vein"` and the registry, as one advisory lock key.
const fn lock_key(kind: AssetKind) -> i64 {
    let registry = match kind {
        AssetKind::Gem | AssetKind::Spec => 1,
        AssetKind::Crate => 2,
        AssetKind::NpmPackage => 3,
    };
    0x7665_696e_0000_0000 | registry
}

#[cfg(test)]
mod tests;
