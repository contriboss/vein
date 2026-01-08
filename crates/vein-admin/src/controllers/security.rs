//! Security vulnerabilities page.

use rama::http::service::web::extract::State;
use rama::http::service::web::response::IntoResponse;
use serde::Serialize;
use tera::Context;

use crate::controllers::render;
use crate::state::AdminState;

#[derive(Debug, Serialize)]
pub struct VulnerableGem {
    pub name: String,
    pub version: String,
    pub cve: String,
    pub severity: String,
    pub patched_in: String,
    pub note: String,
}

pub async fn index(State(state): State<AdminState>) -> impl IntoResponse {
    // TODO: fetch from vulnerability database
    let vulnerabilities: Vec<VulnerableGem> = vec![];

    let mut context = Context::new();
    context.insert("current_page", "security");
    context.insert("vulnerabilities", &vulnerabilities);
    render(&state.tera, "security/index.html", &context)
}
