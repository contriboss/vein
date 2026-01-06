//! Permissions/Entitlements page.

use rama::http::service::web::extract::State;
use rama::http::service::web::response::{Html, IntoResponse};
use tera::Context;

use crate::state::AdminState;

pub async fn index(State(state): State<AdminState>) -> impl IntoResponse {
    let mut context = Context::new();
    context.insert("current_page", "permissions");

    match state.tera.render("permissions/index.html", &context) {
        Ok(html) => Html(html),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render permissions template");
            Html(format!("<h1>Template Error: {}</h1>", e))
        }
    }
}
