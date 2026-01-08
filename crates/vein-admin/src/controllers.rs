//! HTTP request handlers for vein-admin.

use rama::http::service::web::response::Html;
use tera::{Context, Tera};

pub mod catalog;
pub mod changelog;
pub mod dashboard;
pub mod health;
pub mod permissions;
pub mod quarantine;
pub mod security;

/// Render a Tera template with error handling.
pub fn render(tera: &Tera, template: &str, context: &Context) -> Html<String> {
    match tera.render(template, context) {
        Ok(html) => Html(html),
        Err(e) => {
            tracing::error!(error = %e, template = %template, "Template render failed");
            Html(format!("<h1>Template Error: {}</h1>", e))
        }
    }
}
