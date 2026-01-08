//! Error types for vein-admin handlers.
//!
//! Structured error handling for controllers. Currently unused but
//! provides a pattern for typed error responses.

#![allow(dead_code)]

use rama::http::service::web::response::{Html, IntoResponse};
use rama::http::StatusCode;

pub type Result<T> = std::result::Result<T, AdminError>;

#[derive(Debug)]
pub enum AdminError {
    Internal(anyhow::Error),
    NotFound,
    BadRequest(String),
    TemplateError(tera::Error),
}

impl std::fmt::Display for AdminError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Internal(e) => write!(f, "internal error: {}", e),
            Self::NotFound => write!(f, "not found"),
            Self::BadRequest(msg) => write!(f, "bad request: {}", msg),
            Self::TemplateError(e) => write!(f, "template error: {}", e),
        }
    }
}

impl std::error::Error for AdminError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Internal(e) => Some(e.as_ref()),
            Self::TemplateError(e) => Some(e),
            _ => None,
        }
    }
}

impl IntoResponse for AdminError {
    fn into_response(self) -> rama::http::Response {
        match self {
            Self::Internal(e) => {
                tracing::error!(error = %e, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Html("<h1>Internal Server Error</h1>"),
                )
                    .into_response()
            }
            Self::NotFound => (StatusCode::NOT_FOUND, Html("<h1>Not Found</h1>")).into_response(),
            Self::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                Html(format!("<h1>Bad Request: {}</h1>", msg)),
            )
                .into_response(),
            Self::TemplateError(e) => {
                tracing::error!(error = %e, "template error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Html("<h1>Template Error</h1>"),
                )
                    .into_response()
            }
        }
    }
}

impl From<anyhow::Error> for AdminError {
    fn from(e: anyhow::Error) -> Self {
        Self::Internal(e)
    }
}

impl From<tera::Error> for AdminError {
    fn from(e: tera::Error) -> Self {
        Self::TemplateError(e)
    }
}
