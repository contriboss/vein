//! Application bootstrap for vein-admin.

use std::sync::Arc;

use sqlx::sqlite::SqlitePoolOptions;
use tera::Tera;
use tracing::warn;

use crate::{config::AdminConfig, ruby, state::{AdminResources, AdminState}};
use vein::{catalog, config::Config as VeinConfig, db::connect_cache_backend};
use vein_adapter::FilesystemStorage;

/// Bootstrap the application state.
pub async fn bootstrap(config: &AdminConfig) -> anyhow::Result<AdminState> {
    // Load vein config
    let vein_config = Arc::new(VeinConfig::load(Some(config.vein.config_path.clone().into()))?);
    vein_config.validate()?;

    // Prepare storage
    FilesystemStorage::new(vein_config.storage.path.clone())
        .prepare()
        .await
        .ok();

    // Connect cache backend
    let (cache_backend, _) = connect_cache_backend(&vein_config).await?;

    // Connect admin DB (SQLx)
    let db = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&config.database.url)
        .await?;

    // Run migrations
    sqlx::migrate!("./migrations").run(&db).await?;

    // Load templates
    let tera = Arc::new(Tera::new("crates/vein-admin/assets/views/**/*.html")?);

    // Fetch Ruby status
    let ruby_status = match ruby::fetch_ruby_status().await {
        Ok(status) => Arc::new(status),
        Err(err) => {
            warn!(error = %err, "failed to fetch ruby status");
            Arc::new(ruby::RubyStatus::default())
        }
    };

    // Create resources
    let resources = AdminResources::new(vein_config.clone(), cache_backend.clone(), ruby_status);

    // Spawn background sync
    catalog::spawn_background_sync(cache_backend)?;

    Ok(AdminState::new(resources, tera, db))
}
