//! HTTP request handlers for the admin dashboard.

use rama::http::service::web::response::Html;
use rama::telemetry::tracing;
use tera::{Context, Tera};

#[cfg(feature = "gems")]
pub mod catalog;
pub mod dashboard;
pub mod health;
pub mod quarantine;

/// Render a Tera template with error handling.
pub fn render(tera: &Tera, template: &str, context: &Context) -> Html<String> {
    match tera.render(template, context) {
        Ok(html) => Html(html),
        Err(e) => {
            tracing::error!(error = %e, template = %template, "Template render failed");
            Html(format!("<h1>Template Error: {e}</h1>"))
        }
    }
}
