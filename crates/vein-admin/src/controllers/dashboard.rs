//! Dashboard controller with SSE streaming.

use std::time::Duration;

use rama::http::service::web::extract::{Query, State};
use rama::http::service::web::response::{Html, IntoResponse};
use rama::http::sse::Event;
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::state::{AdminResources, AdminState};
use crate::utils::{datastar_patch_event, error_html, sse_from_receiver};
use crate::views;

#[derive(Debug, Deserialize, Default)]
pub struct DashboardQuery {
    #[serde(default)]
    pub upstream: Option<String>,
}

pub async fn index(
    State(state): State<AdminState>,
    Query(query): Query<DashboardQuery>,
) -> impl IntoResponse {
    tracing::info!("Dashboard index requested");

    let data = match load_dashboard_data(&state.resources, query.upstream.is_some()).await {
        Ok(data) => data,
        Err(err) => return error_html(err),
    };

    match views::dashboard::index(&state.tera, data) {
        Ok(html) => Html(html),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render template");
            error_html(format!("Template Error: {e}"))
        }
    }
}

pub async fn stats(State(state): State<AdminState>) -> impl IntoResponse {
    let data = match load_dashboard_data(&state.resources, false).await {
        Ok(data) => data,
        Err(err) => return fragment_error(err),
    };

    match views::dashboard::stats(&state.tera, data) {
        Ok(html) => Html(html),
        Err(e) => fragment_error(e),
    }
}

/// SSE stats stream for live dashboard updates
pub async fn stats_stream(State(state): State<AdminState>) -> impl IntoResponse {
    // Create a channel for events
    let (tx, rx) = mpsc::channel::<Event<String>>(16);

    // Spawn a task to generate events - this runs async operations
    tokio::spawn({
        let resources = state.resources.clone();
        let tera = state.tera.clone();
        async move {
            loop {
                let event = match get_stats_event_inner(&resources, &tera).await {
                    Ok(html) => datastar_patch_event(html),
                    Err(err) => {
                        tracing::error!(error = %err, "failed to get stats for SSE");
                        datastar_patch_event(format!("<div id='stats-error'>Error: {err}</div>"))
                    }
                };

                if tx.send(event).await.is_err() {
                    // Receiver dropped, client disconnected
                    break;
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    });

    sse_from_receiver(rx)
}

async fn get_stats_event_inner(
    resources: &AdminResources,
    tera: &std::sync::Arc<tera::Tera>,
) -> anyhow::Result<String> {
    let data = load_dashboard_data(resources, false).await?;
    views::dashboard::stats_fragment(tera, data)
}

async fn load_dashboard_data(
    resources: &AdminResources,
    show_upstream: bool,
) -> anyhow::Result<views::dashboard::DashboardData> {
    let snapshot = resources.snapshot().await?;
    Ok(views::dashboard::DashboardData::from_snapshot(
        &snapshot,
        show_upstream,
    ))
}

fn fragment_error(err: impl std::fmt::Display) -> Html<String> {
    Html(format!("<div>Error: {}</div>", err))
}
