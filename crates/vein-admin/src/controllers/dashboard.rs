use axum::response::sse::{Event, KeepAlive, Sse};
use loco_rs::prelude::*;
use rama::futures::stream::{self, Stream};
use serde::Deserialize;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tera::Tera;

use super::resources;
use crate::views;

#[derive(Debug, Deserialize, Default)]
struct DashboardQuery {
    #[serde(default)]
    upstream: Option<String>,
}

pub fn routes() -> Routes {
    Routes::new()
        .add("/", get(index))
        .add("/stats", get(stats))
        .add("/stats/stream", get(stats_stream))
}

#[debug_handler]
async fn index(
    State(ctx): State<AppContext>,
    Query(query): Query<DashboardQuery>,
) -> Result<Response> {
    tracing::info!("Dashboard index requested");

    let tera = ctx
        .shared_store
        .get::<Arc<Tera>>()
        .ok_or_else(|| {
            tracing::error!("Tera not available in shared store");
            Error::Message("Tera not available".into())
        })?;

    tracing::info!("Tera retrieved successfully");

    let resources = resources(&ctx).map_err(|e| {
        tracing::error!("Failed to get resources: {:?}", e);
        e
    })?;

    tracing::info!("Resources retrieved successfully");

    let snapshot = resources
        .snapshot()
        .await
        .map_err(|err| {
            tracing::error!("Failed to get snapshot: {:?}", err);
            Error::Message(err.to_string())
        })?;

    tracing::info!("Snapshot retrieved successfully");

    let show_upstream = query.upstream.is_some();
    let data = views::dashboard::DashboardData::from_snapshot(&snapshot, show_upstream);

    tracing::info!("Data created, rendering view");

    views::dashboard::index(&tera, data).map_err(|e| {
        tracing::error!("Failed to render view: {:?}", e);
        e
    })
}

#[debug_handler]
async fn stats(State(ctx): State<AppContext>) -> Result<Response> {
    let tera = ctx
        .shared_store
        .get::<Arc<Tera>>()
        .ok_or_else(|| Error::Message("Tera not available".into()))?;

    let resources = resources(&ctx)?;
    let snapshot = resources
        .snapshot()
        .await
        .map_err(|err| Error::Message(err.to_string()))?;

    let data = views::dashboard::DashboardData::from_snapshot(&snapshot, false);

    views::dashboard::stats(&tera, data)
}

#[debug_handler]
async fn stats_stream(
    State(ctx): State<AppContext>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = stream::unfold(ctx, |ctx| async move {
        // Get current stats
        let event = match get_stats_event(&ctx).await {
            Ok(evt) => evt,
            Err(err) => {
                tracing::error!(error = %err, "failed to get stats for SSE");
                // Return error event
                Event::default()
                    .event("datastar-patch-elements")
                    .data(format!("fragments <div id='stats-error'>Error: {}</div>", err))
            }
        };

        // Wait before next update
        tokio::time::sleep(Duration::from_secs(5)).await;

        Some((Ok(event), ctx))
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn get_stats_event(ctx: &AppContext) -> Result<Event> {
    let tera = ctx
        .shared_store
        .get::<Arc<Tera>>()
        .ok_or_else(|| Error::Message("Tera not available".into()))?;

    let resources = resources(ctx)?;
    let snapshot = resources
        .snapshot()
        .await
        .map_err(|err| Error::Message(err.to_string()))?;

    let data = views::dashboard::DashboardData::from_snapshot(&snapshot, false);

    // Render the stats partial
    let html = views::dashboard::stats_fragment(&tera, data)?;

    // Create Datastar-compatible SSE event
    // Format: event: datastar-patch-elements
    //         data: fragments <html>
    Ok(Event::default()
        .event("datastar-patch-elements")
        .data(format!("fragments {}", html)))
}
