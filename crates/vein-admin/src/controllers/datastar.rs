use loco_rs::prelude::*;
use rama::http::body::util::BodyExt;
use rama::http::service::web::response::{DatastarScript, IntoResponse as RamaIntoResponse};

pub fn routes() -> Routes {
    Routes::new().add("/assets/datastar.js", get(script))
}

/// Serves Datastar v1.0.0-RC.6 from rama's embedded DatastarScript
#[debug_handler]
async fn script() -> Result<Response> {
    // Get rama's response with embedded datastar.js
    let rama_response = DatastarScript::default().into_response();

    // Extract status and headers
    let status = rama_response.status();
    let headers = rama_response.headers().clone();
    let body = rama_response.into_body();

    // Collect body bytes
    let body_bytes = body
        .collect()
        .await
        .map_err(|e| Error::Message(format!("Failed to read datastar body: {}", e)))?
        .to_bytes();

    // Build axum-compatible response
    let mut builder = Response::builder().status(axum::http::StatusCode::from_u16(status.as_u16()).unwrap());

    // Copy headers from rama response to axum response builder
    for (name, value) in headers.iter() {
        builder = builder.header(
            axum::http::HeaderName::from_bytes(name.as_str().as_bytes()).unwrap(),
            axum::http::HeaderValue::from_bytes(value.as_bytes()).unwrap(),
        );
    }

    let response = builder
        .body(body_bytes.into())
        .map_err(|e| Error::Message(e.to_string()))?;

    Ok(response)
}
