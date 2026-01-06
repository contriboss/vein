//! Changelog page.

use rama::http::service::web::extract::State;
use rama::http::service::web::response::{Html, IntoResponse};
use serde::Serialize;
use tera::Context;

use crate::state::AdminState;

#[derive(Debug, Serialize)]
pub struct ChangeLogEntry {
    pub date: String,
    pub title: String,
    pub category: String,
    pub details: String,
    pub highlight: bool,
}

fn sample_entries() -> Vec<ChangeLogEntry> {
    vec![]
}

pub async fn index(State(state): State<AdminState>) -> impl IntoResponse {
    let entries = sample_entries();

    let mut context = Context::new();
    context.insert("current_page", "changelog");
    context.insert("entries", &entries);

    match state.tera.render("changelog/index.html", &context) {
        Ok(html) => Html(html),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render changelog template");
            Html(format!("<h1>Template Error: {}</h1>", e))
        }
    }
}
