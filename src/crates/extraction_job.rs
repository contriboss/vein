#![cfg(feature = "crates")]

//! Crate exports extraction as a stampeded async job.

use rama::telemetry::tracing;
use serde::{Deserialize, Serialize};
use stampeded::async_job::{AsyncJob, JobResult};

use super::exports::{extract_exports, extract_lib_rs_from_crate};

/// Extract public APIs from a Rust crate tarball.
/// Saves results to JSON for processing by a subsequent background task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrateExtractor {
    pub name: String,
    pub version: String,
    pub tarball_path: String,
}

#[derive(Debug, Serialize)]
pub struct ExtractionResult {
    pub exports_count: usize,
    pub exports_json_path: String,
}

impl AsyncJob for CrateExtractor {
    const TASK: &'static str = "crate.extract";
    const QUEUE: &'static str = "vein";
    type Output = ExtractionResult;

    fn run(
        &self,
        _ctx: &stampeded::worker::JobContext,
    ) -> impl std::future::Future<Output = JobResult<Self::Output>> + Send {
        let name = self.name.clone();
        let version = self.version.clone();
        let tarball_path = self.tarball_path.clone();

        async move {
            let tarball_bytes = std::fs::read(&tarball_path).map_err(|e| {
                Box::new(std::io::Error::new(e.kind(), e.to_string()))
                    as Box<dyn std::error::Error + Send + Sync>
            })?;

            // Extract lib.rs from tarball
            let Some(lib_rs) =
                extract_lib_rs_from_crate(&tarball_bytes, &name, &version).map_err(|e| {
                    Box::new(std::io::Error::other(e.to_string()))
                        as Box<dyn std::error::Error + Send + Sync>
                })?
            else {
                tracing::debug!(
                    crate_name = %name,
                    version = %version,
                    "no lib.rs found in crate"
                );
                // Clean up
                let _ = std::fs::remove_file(&tarball_path);
                return Ok(ExtractionResult {
                    exports_count: 0,
                    exports_json_path: String::new(),
                });
            };

            // Parse and extract public APIs
            let exports = extract_exports(&lib_rs, &name).map_err(|e| {
                Box::new(std::io::Error::other(e.to_string()))
                    as Box<dyn std::error::Error + Send + Sync>
            })?;

            let count = exports.len();

            // Save exports to JSON for db insertion
            let exports_path = format!("/tmp/vein/{name}-{version}.exports.json");
            let json = serde_json::to_string(&exports).map_err(|e| {
                Box::new(std::io::Error::other(e.to_string()))
                    as Box<dyn std::error::Error + Send + Sync>
            })?;
            std::fs::write(&exports_path, json).map_err(|e| {
                Box::new(std::io::Error::new(e.kind(), e.to_string()))
                    as Box<dyn std::error::Error + Send + Sync>
            })?;

            // Clean up tarball
            let _ = std::fs::remove_file(&tarball_path);

            tracing::debug!(
                crate_name = %name,
                version = %version,
                exports_count = count,
                exports_path = %exports_path,
                "crate exports extracted"
            );

            Ok(ExtractionResult {
                exports_count: count,
                exports_json_path: exports_path,
            })
        }
    }
}
