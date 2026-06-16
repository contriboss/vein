use std::path::PathBuf;

use anyhow::Result;
use vein_adapter::CacheBackendTrait;

use vein::util::format_bytes;

use super::setup::{build_current_thread_runtime, connect_cache_index, init_tracing, load_config};

pub(crate) fn run_stats(config_path: PathBuf) -> Result<()> {
    let config = load_config(config_path)?;
    init_tracing(&config)?;

    let rt = build_current_thread_runtime("stats")?;
    let (index, backend_kind) = connect_cache_index(&rt, &config)?;
    let index_stats = rt.block_on(index.stats())?;

    #[cfg(feature = "sqlite")]
    println!("SQLite cache: {}", backend_kind.path.display());
    #[cfg(feature = "postgres")]
    println!("PostgreSQL cache: {}", backend_kind.url);
    println!("  total assets: {}", index_stats.total_assets);
    println!("  rubygems assets: {}", index_stats.rubygems_assets);
    println!("  crates assets: {}", index_stats.crate_assets);
    println!("  npm assets: {}", index_stats.npm_assets);
    println!("  unique packages: {}", index_stats.unique_packages);
    println!(
        "  total size: {}",
        format_bytes(index_stats.total_size_bytes)
    );
    if let Some(last) = index_stats.last_accessed {
        println!("  last access: {}", last);
    }

    Ok(())
}
