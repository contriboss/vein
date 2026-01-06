use std::borrow::Cow;

use anyhow::{Context, Result};
use rama::http::service::web::extract::Query;
use rama::http::{Request, StatusCode};
use serde::Deserialize;
use serde_json::{json, to_string_pretty};
use vein_adapter::CacheBackendKind;

use super::response::{respond_json, respond_json_download, respond_text};
use super::types::CacheStatus;
use super::utils::sanitize_filename;

/// Handles health check requests
pub async fn handle_health(
    index: &CacheBackendKind,
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
    index: &CacheBackendKind,
) -> Result<(rama::http::Response<rama::http::Body>, CacheStatus)> {
    let query = req.uri().query().unwrap_or("");

    #[derive(Deserialize, Default)]
    struct Parameters<'a> {
        name: Option<Cow<'a, str>>,
        version: Option<Cow<'a, str>>,
        platform: Option<Cow<'a, str>>,
    }

    // or instead of default return error, which IMHO is probably better?
    let Parameters {
        name,
        version,
        platform,
    } = Query::parse_query_str(query)
        .map(|q| q.0)
        .unwrap_or_default();

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
        sanitize_filename(&meta.platform),
    );

    let resp = respond_json_download(&body, &filename)?;
    Ok((resp, CacheStatus::Hit))
}
