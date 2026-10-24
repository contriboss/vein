//! `GET /.well-known/vein/sbom?name=&version=[&platform=]`: a cached gem's
//! `CycloneDX` SBOM, as a download.

use std::borrow::Cow;

use anyhow::{Context, Result};
use rama::http::service::web::extract::Query;
use rama::http::{Request, StatusCode};
use serde::Deserialize;
use serde_json::to_string_pretty;
use vein_adapter::{CacheBackend, CacheBackendTrait};

use super::response::{respond_json_download, respond_text};
use super::types::CacheStatus;
use super::utils::sanitize_filename;

/// Handles SBOM (Software Bill of Materials) requests
pub async fn handle_sbom_request(
    req: &Request<rama::http::Body>,
    index: &CacheBackend,
) -> Result<(rama::http::Response<rama::http::Body>, CacheStatus)> {
    #[derive(Deserialize, Default)]
    struct Parameters<'a> {
        name: Option<Cow<'a, str>>,
        version: Option<Cow<'a, str>>,
        platform: Option<Cow<'a, str>>,
    }

    let query = req.uri().query_or_empty();

    // or instead of default return error, which IMHO is probably better?
    let Parameters {
        name,
        version,
        platform,
    } = Query::parse_query_str(query.as_ref())
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

    let Some(meta) = metadata.filter(|m| m.sbom.is_some()) else {
        let resp = respond_text(
            StatusCode::NOT_FOUND,
            "SBOM not available for requested gem\n",
        )?;
        return Ok((resp, CacheStatus::Pass));
    };
    let sbom = meta.sbom.as_ref().expect("sbom present after filter");

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
