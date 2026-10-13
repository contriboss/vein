use std::path::PathBuf;

use anyhow::Result;
use rama::tls::rustls::dep::rustls;
use vein::gem_sync;

use super::setup::{
    build_current_thread_runtime, connect_cache_index, init_tracing, load_validated_config,
};

pub fn run_gem_sync(
    config_path: PathBuf,
    name: &str,
    version: Option<&str>,
    platform: Option<&str>,
) -> Result<()> {
    // The upstream client speaks TLS; a second install is not an error.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let config = load_validated_config(config_path)?;
    init_tracing(&config)?;

    let rt = build_current_thread_runtime("gem sync")?;
    let (cache, _) = connect_cache_index(&rt, &config)?;
    rt.block_on(gem_sync::run(&config, &cache, name, version, platform))
}
