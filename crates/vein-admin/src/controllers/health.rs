use loco_rs::prelude::*;

use super::resources;

pub fn routes() -> Routes {
    Routes::new()
        .add("/up", get(up))
        .add("/debug", get(debug))
}

#[debug_handler]
async fn up() -> Result<Response> {
    let response = Response::builder()
        .status(axum::http::StatusCode::OK)
        .header(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )
        .body("ok".into())
        .map_err(|err| Error::Message(err.to_string()))?;

    Ok(response)
}

#[debug_handler]
async fn debug(State(ctx): State<AppContext>) -> Result<Response> {
    let msg = match resources(&ctx) {
        Ok(res) => match res.snapshot().await {
            Ok(_) => "Snapshot OK".to_string(),
            Err(e) => format!("Snapshot error: {:?}", e),
        },
        Err(e) => format!("Resources error: {:?}", e),
    };

    let response = Response::builder()
        .status(axum::http::StatusCode::OK)
        .header(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )
        .body(msg.into())
        .map_err(|err| Error::Message(err.to_string()))?;

    Ok(response)
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn up_returns_ok() {
        let response = super::up().await.expect("up handler");
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/plain; charset=utf-8")
        );
    }
}
