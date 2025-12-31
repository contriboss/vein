use axum::routing::any_service;
use loco_rs::prelude::*;
use tower_http::services::ServeDir;

pub fn routes() -> Routes {
    Routes::new().add(
        "/assets",
        any_service(ServeDir::new("crates/vein-admin/assets/static")),
    )
}
