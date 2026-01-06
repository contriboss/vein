//! HTTP router configuration for vein-admin.

use rama::http::service::web::response::DatastarScript;
use rama::http::service::web::Router;

use crate::controllers;
use crate::state::AdminState;

/// Build the main application router.
pub fn build(state: AdminState) -> Router<AdminState> {
    Router::new_with_state(state)
        // Dashboard
        .with_get("/", controllers::dashboard::index)
        .with_get("/stats", controllers::dashboard::stats)
        .with_get("/stats/stream", controllers::dashboard::stats_stream)
        // Datastar assets (direct from rama - no bridge!)
        .with_get("/assets/datastar.js", DatastarScript::default())
        // Catalog
        .with_get("/catalog", controllers::catalog::list)
        .with_get("/catalog/search", controllers::catalog::search)
        .with_get("/catalog/{name}", controllers::catalog::detail)
        .with_get("/catalog/{name}/sbom", controllers::catalog::sbom)
        // Quarantine
        .with_get("/quarantine", controllers::quarantine::index)
        .with_get("/quarantine/api/stats", controllers::quarantine::api_stats)
        .with_get(
            "/quarantine/api/pending",
            controllers::quarantine::api_pending,
        )
        .with_post(
            "/quarantine/{gem}/{version}/approve",
            controllers::quarantine::approve,
        )
        .with_post(
            "/quarantine/{gem}/{version}/block",
            controllers::quarantine::block,
        )
        // Health
        .with_get("/up", controllers::health::up)
        .with_get("/debug", controllers::health::debug)
        // Static pages
        .with_get("/changelog", controllers::changelog::index)
        .with_get("/permissions", controllers::permissions::index)
        .with_get("/security", controllers::security::index)
}
