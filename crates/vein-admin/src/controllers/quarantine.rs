//! Quarantine management API endpoints.
//!
//! Provides admin UI and API for managing quarantined gem versions:
//! - View quarantine statistics and pending versions
//! - Approve versions for early release
//! - Block malicious versions

use rama::http::service::web::extract::{Form, Path, Query, State};
use rama::http::service::web::response::{Html, IntoResponse, Json, Redirect};
use serde::{Deserialize, Serialize};
use tera::Context;

use crate::controllers::render;
use crate::state::AdminState;

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

pub async fn index(State(state): State<AdminState>) -> impl IntoResponse {
    let mut context = Context::new();
    context.insert("current_page", "quarantine");

    if !state.resources.quarantine_enabled() {
        return render(&state.tera, "quarantine/disabled.html", &context);
    }

    let stats = match state.resources.quarantine_stats().await {
        Ok(s) => QuarantineStats {
            quarantined: s.total_quarantined,
            available: s.total_available,
            pinned: s.total_pinned,
            yanked: s.total_yanked,
        },
        Err(e) => return Html(format!("<h1>Error: {}</h1>", e)),
    };

    let pending = match state.resources.quarantine_pending(50, 0).await {
        Ok(p) => p
            .into_iter()
            .map(|gem| {
                let time_remaining = gem.available_after.signed_duration_since(chrono::Utc::now());
                let hours_remaining = time_remaining.num_hours().max(0);
                let status = match gem.status {
                    vein_adapter::VersionStatus::Quarantine => "Quarantine",
                    vein_adapter::VersionStatus::Available => "Available",
                    vein_adapter::VersionStatus::Yanked => "Yanked",
                    vein_adapter::VersionStatus::Pinned => "Pinned",
                };

                PendingGem {
                    name: gem.name,
                    version: gem.version,
                    platform: gem.platform.clone(),
                    platform_raw: gem.platform.unwrap_or_default(),
                    status: status.to_string(),
                    hours_remaining,
                }
            })
            .collect::<Vec<_>>(),
        Err(e) => return Html(format!("<h1>Error: {}</h1>", e)),
    };

    context.insert("stats", &stats);
    context.insert("pending", &pending);
    render(&state.tera, "quarantine/index.html", &context)
}

pub async fn api_stats(State(state): State<AdminState>) -> impl IntoResponse {
    if !state.resources.quarantine_enabled() {
        return Json(serde_json::json!({
            "enabled": false,
            "error": "Quarantine feature is disabled"
        }));
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
        Err(e) => Json(serde_json::json!({
            "enabled": true,
            "error": e.to_string()
        })),
    }
}

pub async fn api_pending(
    State(state): State<AdminState>,
    Query(query): Query<PendingQuery>,
) -> impl IntoResponse {
    if !state.resources.quarantine_enabled() {
        return Json(serde_json::json!({
            "enabled": false,
            "error": "Quarantine feature is disabled"
        }));
    }

    let limit = query.limit.unwrap_or(50).min(100);
    let offset = query.offset.unwrap_or(0);

    match state.resources.quarantine_pending(limit, offset).await {
        Ok(pending) => {
            let versions: Vec<_> = pending
                .into_iter()
                .map(|v| {
                    serde_json::json!({
                        "name": v.name,
                        "version": v.version,
                        "platform": v.platform,
                        "status": format!("{:?}", v.status),
                        "published_at": v.published_at.to_rfc3339(),
                        "available_after": v.available_after.to_rfc3339(),
                    })
                })
                .collect();

            Json(serde_json::json!({
                "enabled": true,
                "versions": versions,
            }))
        }
        Err(e) => Json(serde_json::json!({
            "enabled": true,
            "error": e.to_string()
        })),
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

    let platform = form.platform.as_deref().filter(|p| !p.is_empty());
    let reason = form.reason.as_deref().unwrap_or("admin approval");

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

    let platform = form.platform.as_deref().filter(|p| !p.is_empty());
    let reason = form.reason.as_deref().unwrap_or("admin blocked");

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
