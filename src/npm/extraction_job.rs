#![cfg(feature = "npm")]

//! NPM package exports extraction as a stampeded async job.

use rama::telemetry::tracing;
use serde::{Deserialize, Serialize};
use stampeded::async_job::{AsyncJob, JobResult};

/// Extract public APIs from an NPM package tarball.
/// Saves results to JSON for processing by a subsequent background task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmExtractor {
    pub name: String,
    pub version: String,
    pub tarball_path: String,
}

#[derive(Debug, Serialize)]
pub struct ExtractionResult {
    pub exports_count: usize,
    pub exports_json_path: String,
}

impl AsyncJob for NpmExtractor {
    const TASK: &'static str = "npm.extract";
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

            // Extract package.json to determine module type
            let _package_json = extract_package_json(&tarball_bytes, &name, &version);

            // For MVP, count declarations from TypeScript definitions or CommonJS
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
                package_name = %name,
                version = %version,
                exports_count = exports_count,
                exports_path = %exports_path,
                "npm package exports extracted"
            );

            Ok(ExtractionResult {
                exports_count,
                exports_json_path: exports_path,
            })
        }
    }
}

fn extract_package_json(_tarball_bytes: &[u8], _name: &str, _version: &str) -> String {
    // TODO: implement tar extraction and JSON parsing
    "{}".to_string()
}
