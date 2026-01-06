//! Dashboard controller with SSE streaming.

use std::time::Duration;

use rama::futures::StreamExt;
use rama::http::service::web::extract::{Query, State};
use rama::http::service::web::response::{Html, IntoResponse, Sse};
use rama::http::sse::server::{KeepAlive, KeepAliveStream};
use rama::http::sse::Event;
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::state::AdminState;
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

    let snapshot = match state.resources.snapshot().await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "Failed to get snapshot");
            return Html(format!("<h1>Error: {}</h1>", e));
        }
    };

    let show_upstream = query.upstream.is_some();
    let data = views::dashboard::DashboardData::from_snapshot(&snapshot, show_upstream);

    match views::dashboard::index(&state.tera, data) {
        Ok(html) => Html(html),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render template");
            Html(format!("<h1>Template Error: {}</h1>", e))
        }
    }
}

pub async fn stats(State(state): State<AdminState>) -> impl IntoResponse {
    let snapshot = match state.resources.snapshot().await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "Failed to get snapshot");
            return Html(format!("<div>Error: {}</div>", e));
        }
    };

    let data = views::dashboard::DashboardData::from_snapshot(&snapshot, false);

    match views::dashboard::stats(&state.tera, data) {
        Ok(html) => Html(html),
        Err(e) => Html(format!("<div>Error: {}</div>", e)),
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
                    Ok(html) => Event::default()
                        .try_with_event("datastar-patch-elements")
                        .expect("valid event name")
                        .with_data(format!("fragments {}", html)),
                    Err(err) => {
                        tracing::error!(error = %err, "failed to get stats for SSE");
                        Event::default()
                            .try_with_event("datastar-patch-elements")
                            .expect("valid event name")
                            .with_data(format!(
                                "fragments <div id='stats-error'>Error: {}</div>",
                                err
                            ))
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

    // Convert receiver to a stream that can be used with Sse
    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    Sse::new(KeepAliveStream::new(
        KeepAlive::new(),
        stream.map(|event| Ok::<_, std::convert::Infallible>(event)),
    ))
}

async fn get_stats_event_inner(
    resources: &crate::state::AdminResources,
    tera: &std::sync::Arc<tera::Tera>,
) -> anyhow::Result<String> {
    let snapshot = resources.snapshot().await?;
    let data = views::dashboard::DashboardData::from_snapshot(&snapshot, false);
    views::dashboard::stats_fragment(tera, data)
}

