//! Quarantine management: the versions the release delay holds, in every
//! registry.
//!
//! - View quarantine statistics and pending versions
//! - Approve versions for early release
//! - Block malicious versions (hidden, downloads refused)

use jiff::Timestamp;
use rama::http::service::web::extract::{Form, Query, State};
use rama::http::service::web::response::{Html, IntoResponse, Json, Redirect};
use rama::telemetry::tracing;
use serde::{Deserialize, Serialize};
use tera::Context;
use vein_adapter::{
    AssetKey, AssetKind, PackageVersion, QuarantineStats as AdapterQuarantineStats, VersionStatus,
};

use crate::admin::controllers::render;
use crate::admin::state::{AdminResources, AdminState};
use crate::admin::utils::error_html;

const DEFAULT_PENDING_LIMIT: u32 = 50;
const MAX_PENDING_LIMIT: u32 = 100;
const QUARANTINE_DISABLED: &str = "Quarantine feature is disabled";
const DEFAULT_APPROVAL_REASON: &str = "admin approval";
const DEFAULT_BLOCK_REASON: &str = "admin blocked";

/// One version and the decision's reason; the name travels in the form
/// rather than the path, as a scoped npm name holds a `/`.
#[derive(Debug, Deserialize)]
pub struct ActionForm {
    registry: AssetKind,
    name: String,
    version: String,
    platform: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PendingQuery {
    limit: Option<u32>,
    offset: Option<u32>,
}

#[derive(Debug, Serialize)]
struct QuarantineStats {
    quarantined: u64,
    available: u64,
    pinned: u64,
    blocked: u64,
}

#[derive(Debug, Serialize)]
struct PendingVersion {
    registry: &'static str,
    name: String,
    version: String,
    platform: Option<String>,
    platform_raw: String,
    status: String,
    hours_remaining: i64,
}

impl ActionForm {
    fn key(&self) -> AssetKey<'_> {
        AssetKey {
            kind: self.registry,
            name: &self.name,
            version: &self.version,
            platform: self.platform.as_deref().filter(|p| !p.is_empty()),
        }
    }

    fn reason_or<'a>(&'a self, fallback: &'a str) -> &'a str {
        self.reason.as_deref().unwrap_or(fallback)
    }
}

impl PendingQuery {
    fn limit(&self) -> u32 {
        self.limit
            .unwrap_or(DEFAULT_PENDING_LIMIT)
            .min(MAX_PENDING_LIMIT)
    }

    fn offset(&self) -> u32 {
        self.offset.unwrap_or(0)
    }
}

impl From<AdapterQuarantineStats> for QuarantineStats {
    fn from(stats: AdapterQuarantineStats) -> Self {
        Self {
            quarantined: stats.total_quarantined,
            available: stats.total_available,
            pinned: stats.total_pinned,
            blocked: stats.total_blocked,
        }
    }
}

impl From<PackageVersion> for PendingVersion {
    fn from(gem: PackageVersion) -> Self {
        let time_remaining = gem.available_after.duration_since(Timestamp::now());
        Self {
            registry: gem.kind.as_str(),
            name: gem.name,
            version: gem.version,
            platform: gem.platform.clone(),
            platform_raw: gem.platform.unwrap_or_default(),
            status: status_label(gem.status).to_string(),
            hours_remaining: time_remaining.as_hours().max(0),
        }
    }
}

pub async fn index(State(state): State<AdminState>) -> impl IntoResponse {
    if !state.resources.quarantine_enabled() {
        return render_disabled(&state.tera);
    }

    let stats = match load_quarantine_stats(&state.resources).await {
        Ok(stats) => stats,
        Err(err) => return error_html(err),
    };
    let pending = match load_pending_versions(&state.resources, DEFAULT_PENDING_LIMIT, 0).await {
        Ok(pending) => pending,
        Err(err) => return error_html(err),
    };

    let mut context = quarantine_context();
    context.insert("stats", &stats);
    context.insert("pending", &pending);
    render(&state.tera, "quarantine/index.html", &context)
}

pub async fn api_stats(State(state): State<AdminState>) -> impl IntoResponse {
    if !state.resources.quarantine_enabled() {
        return Json(disabled_payload());
    }

    match state.resources.quarantine_stats().await {
        Ok(stats) => Json(serde_json::json!({
            "enabled": true,
            "quarantined": stats.total_quarantined,
            "available": stats.total_available,
            "pinned": stats.total_pinned,
            "blocked": stats.total_blocked,
            "releasing_today": stats.versions_releasing_today,
            "releasing_this_week": stats.versions_releasing_this_week,
        })),
        Err(e) => Json(error_payload(e)),
    }
}

pub async fn api_pending(
    State(state): State<AdminState>,
    Query(query): Query<PendingQuery>,
) -> impl IntoResponse {
    if !state.resources.quarantine_enabled() {
        return Json(disabled_payload());
    }

    match state
        .resources
        .quarantine_pending(query.limit(), query.offset())
        .await
    {
        Ok(pending) => Json(serde_json::json!({
            "enabled": true,
            "versions": pending_payload(pending),
        })),
        Err(e) => Json(error_payload(e)),
    }
}

pub async fn approve(
    State(state): State<AdminState>,
    Form(form): Form<ActionForm>,
) -> impl IntoResponse {
    if !state.resources.quarantine_enabled() {
        return Redirect::to("/quarantine");
    }

    let reason = form.reason_or(DEFAULT_APPROVAL_REASON);
    match state.resources.approve_version(&form.key(), reason).await {
        Ok(()) => tracing::info!(
            registry = form.registry.as_str(),
            name = %form.name,
            version = %form.version,
            reason,
            "Version approved"
        ),
        Err(e) => tracing::error!(
            error = %e,
            registry = form.registry.as_str(),
            name = %form.name,
            version = %form.version,
            "Failed to approve version"
        ),
    }

    Redirect::to("/quarantine")
}

pub async fn block(
    State(state): State<AdminState>,
    Form(form): Form<ActionForm>,
) -> impl IntoResponse {
    if !state.resources.quarantine_enabled() {
        return Redirect::to("/quarantine");
    }

    let reason = form.reason_or(DEFAULT_BLOCK_REASON);
    match state.resources.block_version(&form.key(), reason).await {
        Ok(()) => tracing::warn!(
            registry = form.registry.as_str(),
            name = %form.name,
            version = %form.version,
            reason,
            "Version blocked"
        ),
        Err(e) => tracing::error!(
            error = %e,
            registry = form.registry.as_str(),
            name = %form.name,
            version = %form.version,
            "Failed to block version"
        ),
    }

    Redirect::to("/quarantine")
}

async fn load_quarantine_stats(resources: &AdminResources) -> anyhow::Result<QuarantineStats> {
    Ok(resources.quarantine_stats().await?.into())
}

async fn load_pending_versions(
    resources: &AdminResources,
    limit: u32,
    offset: u32,
) -> anyhow::Result<Vec<PendingVersion>> {
    Ok(resources
        .quarantine_pending(limit, offset)
        .await?
        .into_iter()
        .map(PendingVersion::from)
        .collect())
}

fn quarantine_context() -> Context {
    let mut context = Context::new();
    context.insert("current_page", "quarantine");
    context
}

fn render_disabled(tera: &tera::Tera) -> Html<String> {
    render(tera, "quarantine/disabled.html", &quarantine_context())
}

fn disabled_payload() -> serde_json::Value {
    serde_json::json!({
        "enabled": false,
        "error": QUARANTINE_DISABLED
    })
}

fn error_payload(err: impl std::fmt::Display) -> serde_json::Value {
    serde_json::json!({
        "enabled": true,
        "error": err.to_string()
    })
}

fn pending_payload(pending: Vec<PackageVersion>) -> Vec<serde_json::Value> {
    pending
        .into_iter()
        .map(|version| {
            serde_json::json!({
                "registry": version.kind.as_str(),
                "name": version.name,
                "version": version.version,
                "platform": version.platform,
                "status": version.status.to_string(),
                "published_at": version.published_at.to_string(),
                "available_after": version.available_after.to_string(),
            })
        })
        .collect()
}

const fn status_label(status: VersionStatus) -> &'static str {
    match status {
        VersionStatus::Quarantine => "Quarantine",
        VersionStatus::Available => "Available",
        VersionStatus::Blocked => "Blocked",
        VersionStatus::Pinned => "Pinned",
    }
}
