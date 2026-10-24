//! Asking this vein's own proxy, over loopback: what the admin side and the
//! docs need from a registry goes through the proxy, which fetches, applies
//! the delay policy, and caches, exactly as for a client.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use rama::Layer;
use rama::http::{
    HeaderMap, StatusCode, body::util::BodyExt, client::EasyHttpWebClient,
    layer::timeout::TimeoutLayer, service::client::HttpClientExt,
};

use crate::config::ServerConfig;

/// How long a lookup through the proxy may take: its upstream timeout is
/// 30s at most, plus the fallbacks.
const TIMEOUT: Duration = Duration::from_secs(60);

/// `GET path` on the proxy: its headers and body, `None` on 404. `npm`
/// sends the header npm does, so the proxy answers as npm's registry.
pub async fn get(
    server: &ServerConfig,
    path: &str,
    npm: bool,
) -> Result<Option<(HeaderMap, Vec<u8>)>> {
    let url = format!("{}{path}", base(server));
    let client = TimeoutLayer::new(TIMEOUT).into_layer(EasyHttpWebClient::default());
    let mut request = client.get(url.as_str());
    if npm {
        request = request.header("npm-command", "view");
    }
    let response = request
        .send()
        .await
        .map_err(|err| anyhow::anyhow!("asking the proxy for {path}: {err}"))?;
    match response.status() {
        status if status.is_success() => {
            let headers = response.headers().clone();
            let body = response
                .into_body()
                .collect()
                .await
                .with_context(|| format!("reading the proxy's answer for {path}"))?;
            Ok(Some((headers, body.to_bytes().to_vec())))
        }
        StatusCode::NOT_FOUND => Ok(None),
        status => bail!("the proxy answered {status} for {path}"),
    }
}

/// The proxy listener, as this process can reach it.
fn base(server: &ServerConfig) -> String {
    let host = match server.host.as_str() {
        "" | "0.0.0.0" => "127.0.0.1",
        "::" | "[::]" => "[::1]",
        host => host,
    };
    format!("http://{host}:{}", server.port)
}
