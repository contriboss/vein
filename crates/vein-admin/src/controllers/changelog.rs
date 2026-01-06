//! Changelog page.

use rama::http::service::web::extract::State;
use rama::http::service::web::response::IntoResponse;
use serde::Serialize;
use tera::Context;

use crate::controllers::render;
use crate::state::AdminState;

#[derive(Debug, Serialize)]
pub struct ChangeLogEntry {
    pub date: String,
    pub title: String,
    pub category: String,
    pub details: String,
    pub highlight: bool,
}

pub async fn index(State(state): State<AdminState>) -> impl IntoResponse {
    // TODO: fetch from real changelog source
    let entries: Vec<ChangeLogEntry> = vec![];

    let mut context = Context::new();
    context.insert("current_page", "changelog");
    context.insert("entries", &entries);
    render(&state.tera, "changelog/index.html", &context)
}
