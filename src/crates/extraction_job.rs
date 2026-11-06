#![cfg(feature = "crates")]

//! Crate exports extraction as a stampeded async job.

use std::path::Path;

use anyhow::{Context, Result};
use rama::telemetry::tracing;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use stampeded::async_job::{AsyncJob, JobResult};

use super::db::insert_crate_exports;
use super::exports::{extract_exports, extract_lib_rs_from_crate};

/// Index a downloaded crate's public API into `package_exports`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrateExtractor {
    pub name: String,
    pub version: String,
    /// The `.crate` copy the proxy saved for this job; removed once indexed.
    pub tarball_path: String,
}

#[derive(Debug, Serialize)]
pub struct ExtractionResult {
    pub exports_count: usize,
}

impl AsyncJob for CrateExtractor {
    const TASK: &'static str = "crate.extract";
    const QUEUE: &'static str = "vein";
    type Output = ExtractionResult;

    fn run(
        &self,
        ctx: &stampeded::worker::JobContext,
    ) -> impl std::future::Future<Output = JobResult<Self::Output>> + Send {
        let pool = ctx.pool().clone();
        let job = self.clone();
        async move {
            let exports_count =
                extract_into(&pool, &job.name, &job.version, Path::new(&job.tarball_path)).await?;
            Ok(ExtractionResult { exports_count })
        }
    }
}

/// Stores the exports of `tarball`'s `src/lib.rs` and removes the tarball;
/// how many. A failure keeps the tarball for the job's retry.
pub async fn extract_into(
    pool: &PgPool,
    name: &str,
    version: &str,
    tarball: &Path,
) -> Result<usize> {
    let bytes = tokio::fs::read(tarball)
        .await
        .with_context(|| format!("reading {}", tarball.display()))?;
    let exports = if let Some(lib_rs) = extract_lib_rs_from_crate(&bytes, name, version)? {
        extract_exports(&lib_rs, name)?
    } else {
        tracing::debug!(crate_name = %name, %version, "no lib.rs in crate");
        Vec::new()
    };
    let count = exports.len();
    insert_crate_exports(pool, name, version, exports).await?;

    if let Err(err) = tokio::fs::remove_file(tarball).await {
        tracing::warn!(path = %tarball.display(), error = %err, "could not remove indexed crate");
    }
    tracing::debug!(crate_name = %name, %version, exports_count = count, "crate exports indexed");
    Ok(count)
}

#[cfg(test)]
mod tests;
