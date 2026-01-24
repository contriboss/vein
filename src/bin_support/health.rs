use std::time::Duration;

use anyhow::{Context, Result, bail};
use rama::Layer;
use rama::http::client::EasyHttpWebClient;
use rama::http::layer::timeout::TimeoutLayer;
use rama::http::service::client::HttpClientExt;

use super::setup::build_current_thread_runtime;

pub(crate) fn run_health(url: String, timeout: u64) -> Result<()> {
    let rt = build_current_thread_runtime("health check")?;

    rt.block_on(async {
        let client = TimeoutLayer::new(Duration::from_secs(timeout))
            .into_layer(EasyHttpWebClient::default());

        let response = client
            .get(&url)
            .send()
            .await
            .context("sending health check request")?;

        if response.status().is_success() {
            println!("Vein healthy: {}", response.status());
            Ok(())
        } else {
            bail!("health endpoint returned status {}", response.status());
        }
    })
}
