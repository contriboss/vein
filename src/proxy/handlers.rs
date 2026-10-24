use anyhow::Result;
use rama::http::StatusCode;
use serde_json::json;
use vein_adapter::{CacheBackend, CacheBackendTrait};

use super::response::respond_json;
use super::types::CacheStatus;

/// Handles health check requests
pub async fn handle_health(
    index: &CacheBackend,
) -> Result<(rama::http::Response<rama::http::Body>, CacheStatus)> {
    let mut ok = true;
    let mut checks = Vec::new();

    match index.stats().await {
        Ok(stats) => {
            checks.push(json!({
                "component": "cache_index",
                "status": "ok",
                "total_assets": stats.total_assets,
                "unique_packages": stats.unique_packages,
                "rubygems_assets": stats.rubygems_assets,
                "crates_assets": stats.crate_assets,
                "npm_assets": stats.npm_assets,
            }));
        }
        Err(err) => {
            ok = false;
            checks.push(json!({
                "component": "cache_index",
                "status": "error",
                "error": err.to_string()
            }));
        }
    }

    let status = if ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    let body = json!({
        "status": if ok { "ok" } else { "degraded" },
        "checks": checks,
    });

    let resp = respond_json(status, &body.to_string())?;
    Ok((
        resp,
        if ok {
            CacheStatus::Pass
        } else {
            CacheStatus::Error
        },
    ))
}
