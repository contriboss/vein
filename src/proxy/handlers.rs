use anyhow::{Context, Result};
use rama::http::{Request, StatusCode};
use serde_json::{json, to_string_pretty};
use url::form_urlencoded;
use vein_adapter::CacheBackend;

use super::response::{respond_json, respond_json_download, respond_text};
use super::types::CacheStatus;
use super::utils::sanitize_filename;

/// Handles health check requests
pub async fn handle_health(
    index: &dyn CacheBackend,
) -> Result<(rama::http::Response<rama::http::Body>, CacheStatus)> {
    let mut ok = true;
    let mut checks = Vec::new();

    match index.stats().await {
        Ok(stats) => {
            checks.push(json!({
                "component": "cache_index",
                "status": "ok",
                "total_assets": stats.total_assets,
                "unique_gems": stats.unique_gems,
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

/// Handles SBOM (Software Bill of Materials) requests
pub async fn handle_sbom_request(
    req: &Request<rama::http::Body>,
    index: &dyn CacheBackend,
) -> Result<(rama::http::Response<rama::http::Body>, CacheStatus)> {
    let query = req.uri().query().unwrap_or("");
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut platform: Option<String> = None;

    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        match key.as_ref() {
            "name" => name = Some(trimmed.to_string()),
            "version" => version = Some(trimmed.to_string()),
            "platform" => platform = Some(trimmed.to_string()),
            _ => {}
        }
    }

    let Some(name) = name else {
        let resp = respond_text(
            StatusCode::BAD_REQUEST,
            "query parameter 'name' is required\n",
        )?;
        return Ok((resp, CacheStatus::Pass));
    };

    let Some(version) = version else {
        let resp = respond_text(
            StatusCode::BAD_REQUEST,
            "query parameter 'version' is required\n",
        )?;
        return Ok((resp, CacheStatus::Pass));
    };

    let platform = platform;

    let mut metadata = index
        .gem_metadata(&name, &version, platform.as_deref())
        .await
        .context("loading cached gem metadata for SBOM request")?;

    if metadata.is_none() && platform.is_none() {
        metadata = index
            .gem_metadata(&name, &version, Some("ruby"))
            .await
            .context("retrying SBOM lookup for ruby platform")?;
    }

    let Some(meta) = metadata else {
        let resp = respond_text(
            StatusCode::NOT_FOUND,
            "SBOM not available for requested gem\n",
        )?;
        return Ok((resp, CacheStatus::Pass));
    };

    let Some(sbom) = meta.sbom.as_ref() else {
        let resp = respond_text(
            StatusCode::NOT_FOUND,
            "SBOM not available for requested gem\n",
        )?;
        return Ok((resp, CacheStatus::Pass));
    };

    let body = to_string_pretty(sbom).context("serializing SBOM JSON payload")?;

    let filename = format!(
        "{}-{}-{}.sbom.json",
        sanitize_filename(&meta.name),
        sanitize_filename(&meta.version),
        sanitize_filename(meta.platform.as_deref().unwrap_or("ruby")),
    );

    let resp = respond_json_download(&body, &filename)?;
    Ok((resp, CacheStatus::Hit))
}
