use std::path::PathBuf;

use anyhow::Result;
use vein::catalog;
use vein_adapter::AssetKind;

use super::setup::{build_current_thread_runtime, connect_cache_index, init_tracing, load_config};

/// Syncs `registry`'s names, or every registry the config keeps a catalog
/// of.
pub fn run_catalog_sync(config_path: PathBuf, registry: Option<AssetKind>) -> Result<()> {
    let config = load_config(config_path)?;
    init_tracing(&config)?;

    let rt = build_current_thread_runtime("catalog")?;
    let (index, _) = connect_cache_index(&rt, &config)?;

    let registries = match registry {
        Some(kind) => vec![vein::registry::require(kind)?],
        None => config.catalog.registries.clone(),
    };
    for kind in registries {
        match rt.block_on(catalog::sync_once(index.as_ref(), kind)) {
            Ok(Some(count)) => println!("{} catalogue synced: {count} names", kind.as_str()),
            Ok(None) => println!("{} catalogue already up to date", kind.as_str()),
            Err(err) => return Err(err.context(format!("syncing the {} catalogue", kind.as_str()))),
        }
    }
    Ok(())
}
