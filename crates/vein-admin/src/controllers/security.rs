//! Security vulnerabilities page.

use rama::http::service::web::extract::State;
use rama::http::service::web::response::{Html, IntoResponse};
use serde::Serialize;
use tera::Context;

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

fn sample_vulnerabilities() -> Vec<VulnerableGem> {
    vec![]
}

pub async fn index(State(state): State<AdminState>) -> impl IntoResponse {
    let vulnerabilities = sample_vulnerabilities();

    let mut context = Context::new();
    context.insert("current_page", "security");
    context.insert("vulnerabilities", &vulnerabilities);

    match state.tera.render("security/index.html", &context) {
        Ok(html) => Html(html),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render security template");
            Html(format!("<h1>Template Error: {}</h1>", e))
        }
    }
}
