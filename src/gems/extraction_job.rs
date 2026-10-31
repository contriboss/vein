#![cfg(feature = "gems")]

//! Gem exports extraction as a stampeded async job.

use rama::telemetry::tracing;
use serde::{Deserialize, Serialize};
use stampeded::async_job::{AsyncJob, JobResult};

/// Extract public APIs from a Ruby gem.
/// Saves results to JSON for processing by a subsequent background task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GemExtractor {
    pub name: String,
    pub version: String,
    pub tarball_path: String,
}

#[derive(Debug, Serialize)]
pub struct ExtractionResult {
    pub exports_count: usize,
    pub exports_json_path: String,
}

impl AsyncJob for GemExtractor {
    const TASK: &'static str = "gem.extract";
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
            let _tarball_bytes = std::fs::read(&tarball_path).map_err(|e| {
                Box::new(std::io::Error::new(e.kind(), e.to_string()))
                    as Box<dyn std::error::Error + Send + Sync>
            })?;

            // For MVP, return stub results
            // TODO: parse .gemspec or lib/*.rb with tree-sitter-ruby
            let exports_count = 0;
            let exports_path = format!("/tmp/vein/{name}-{version}.exports.json");

            // Write stub exports
            let json = "[]";
            std::fs::write(&exports_path, json).map_err(|e| {
                Box::new(std::io::Error::new(e.kind(), e.to_string()))
                    as Box<dyn std::error::Error + Send + Sync>
            })?;

            // Clean up tarball
            let _ = std::fs::remove_file(&tarball_path);

            tracing::debug!(
                gem_name = %name,
                version = %version,
                exports_count = exports_count,
                exports_path = %exports_path,
                "gem exports extracted"
            );

            Ok(ExtractionResult {
                exports_count,
                exports_json_path: exports_path,
            })
        }
    }
}
