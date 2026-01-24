//! Quarantine management API endpoints.
//!
//! Provides admin UI and API for managing quarantined gem versions:
//! - View quarantine statistics and pending versions
//! - Approve versions for early release
//! - Block malicious versions

use chrono::Utc;
use rama::http::service::web::extract::{Form, Path, Query, State};
use rama::http::service::web::response::{Html, IntoResponse, Json, Redirect};
use serde::{Deserialize, Serialize};
use tera::Context;
use vein_adapter::{GemVersion, QuarantineStats as AdapterQuarantineStats, VersionStatus};

use crate::controllers::render;
use crate::state::{AdminResources, AdminState};

const DEFAULT_PENDING_LIMIT: u32 = 50;
const MAX_PENDING_LIMIT: u32 = 100;
const QUARANTINE_DISABLED: &str = "Quarantine feature is disabled";
const DEFAULT_APPROVAL_REASON: &str = "admin approval";
const DEFAULT_BLOCK_REASON: &str = "admin blocked";

#[derive(Debug, Deserialize)]
pub struct ActionForm {
    reason: Option<String>,
    platform: Option<String>,
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
    yanked: u64,
}

#[derive(Debug, Serialize)]
struct PendingGem {
    name: String,
    version: String,
    platform: Option<String>,
    platform_raw: String,
    status: String,
    hours_remaining: i64,
}

impl ActionForm {
    fn platform(&self) -> Option<&str> {
        self.platform.as_deref().filter(|p| !p.is_empty())
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
            yanked: stats.total_yanked,
        }
    }
}

impl From<GemVersion> for PendingGem {
    fn from(gem: GemVersion) -> Self {
        let time_remaining = gem.available_after.signed_duration_since(Utc::now());
        Self {
            name: gem.name,
            version: gem.version,
            platform: gem.platform.clone(),
            platform_raw: gem.platform.unwrap_or_default(),
            status: status_label(gem.status).to_string(),
            hours_remaining: time_remaining.num_hours().max(0),
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
            "yanked": stats.total_yanked,
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
    Path((gem, version)): Path<(String, String)>,
    Form(form): Form<ActionForm>,
) -> impl IntoResponse {
    if !state.resources.quarantine_enabled() {
        return Redirect::to("/quarantine");
    }

    let platform = form.platform();
    let reason = form.reason_or(DEFAULT_APPROVAL_REASON);

    match state
        .resources
        .approve_version(&gem, &version, platform, reason)
        .await
    {
        Ok(()) => {
            tracing::info!(gem = %gem, version = %version, reason = %reason, "Version approved");
        }
        Err(e) => {
            tracing::error!(error = %e, gem = %gem, version = %version, "Failed to approve version");
        }
    }

    Redirect::to("/quarantine")
}

pub async fn block(
    State(state): State<AdminState>,
    Path((gem, version)): Path<(String, String)>,
    Form(form): Form<ActionForm>,
) -> impl IntoResponse {
    if !state.resources.quarantine_enabled() {
        return Redirect::to("/quarantine");
    }

    let platform = form.platform();
    let reason = form.reason_or(DEFAULT_BLOCK_REASON);

    match state
        .resources
        .block_version(&gem, &version, platform, reason)
        .await
    {
        Ok(()) => {
            tracing::warn!(gem = %gem, version = %version, reason = %reason, "Version blocked");
        }
        Err(e) => {
            tracing::error!(error = %e, gem = %gem, version = %version, "Failed to block version");
        }
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
) -> anyhow::Result<Vec<PendingGem>> {
    Ok(resources
        .quarantine_pending(limit, offset)
        .await?
        .into_iter()
        .map(PendingGem::from)
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

fn pending_payload(pending: Vec<GemVersion>) -> Vec<serde_json::Value> {
    pending
        .into_iter()
        .map(|version| {
            serde_json::json!({
                "name": version.name,
                "version": version.version,
                "platform": version.platform,
                "status": format!("{:?}", version.status),
                "published_at": version.published_at.to_rfc3339(),
                "available_after": version.available_after.to_rfc3339(),
            })
        })
        .collect()
}

fn status_label(status: VersionStatus) -> &'static str {
    match status {
        VersionStatus::Quarantine => "Quarantine",
        VersionStatus::Available => "Available",
        VersionStatus::Yanked => "Yanked",
        VersionStatus::Pinned => "Pinned",
    }
}

fn error_html(err: impl std::fmt::Display) -> Html<String> {
    Html(format!("<h1>Error: {}</h1>", err))
}
