//! Health check endpoints.

use rama::http::service::web::extract::State;
use rama::http::service::web::response::IntoResponse;
use rama::http::StatusCode;

use crate::state::AdminState;

pub async fn up() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("content-type", "text/plain; charset=utf-8")],
        "ok",
    )
}

pub async fn debug(State(state): State<AdminState>) -> impl IntoResponse {
    let msg = match state.resources.snapshot().await {
        Ok(_) => "Snapshot OK".to_string(),
        Err(e) => format!("Snapshot error: {:?}", e),
    };

    (
        StatusCode::OK,
        [("content-type", "text/plain; charset=utf-8")],
        msg,
    )
}
