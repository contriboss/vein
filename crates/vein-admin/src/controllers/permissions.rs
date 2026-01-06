//! Permissions/Entitlements page.

use rama::http::service::web::extract::State;
use rama::http::service::web::response::IntoResponse;
use tera::Context;

use crate::controllers::render;
use crate::state::AdminState;

pub async fn index(State(state): State<AdminState>) -> impl IntoResponse {
    let mut context = Context::new();
    context.insert("current_page", "permissions");
    render(&state.tera, "permissions/index.html", &context)
}
