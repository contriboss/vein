use std::path::PathBuf;

use anyhow::Result;
use vein::catalog;

use super::setup::{build_current_thread_runtime, connect_cache_index, init_tracing, load_config};

pub(crate) fn run_catalog_sync(config_path: PathBuf) -> Result<()> {
    let config = load_config(config_path)?;
    init_tracing(&config)?;

    let rt = build_current_thread_runtime("catalog")?;
    let (index, _) = connect_cache_index(&rt, &config)?;

    match rt.block_on(catalog::sync_names_once(index.as_ref())) {
        Ok(Some(count)) => {
            println!("Catalogue synced: {} gem names processed", count);
            Ok(())
        }
        Ok(None) => {
            println!("Catalogue already up to date");
            Ok(())
        }
        Err(err) => Err(err.context("syncing catalogue")),
    }
}
