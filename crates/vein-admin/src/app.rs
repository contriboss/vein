//! Application bootstrap for vein-admin.

use std::sync::Arc;

use anyhow::{Context, Result};
use sqlx::sqlite::SqlitePoolOptions;
use tera::Tera;
use tracing::warn;

use crate::{
    config::AdminConfig,
    ruby,
    state::{AdminResources, AdminState},
};
use vein::{catalog, config::Config as VeinConfig, db::connect_cache_backend};
use vein_adapter::{CacheBackend, FilesystemStorage};

const TEMPLATE_GLOB: &str = "crates/vein-admin/assets/views/**/*.html";

struct VeinRuntime {
    config: Arc<VeinConfig>,
    cache: Arc<CacheBackend>,
}

struct AdminServices {
    tera: Arc<Tera>,
    ruby_status: Arc<ruby::RubyStatus>,
}

/// Bootstrap the application state.
pub async fn bootstrap(config: &AdminConfig) -> Result<AdminState> {
    let runtime = bootstrap_vein_runtime(config).await?;
    let services = initialize_admin_services(config).await?;
    spawn_background_jobs(runtime.cache.clone())?;
    Ok(build_admin_state(runtime, services))
}

async fn bootstrap_vein_runtime(config: &AdminConfig) -> Result<VeinRuntime> {
    let vein_config = load_vein_config(config)?;
    prepare_storage(vein_config.as_ref()).await?;
    let cache = connect_cache(&vein_config).await?;

    Ok(VeinRuntime {
        config: vein_config,
        cache,
    })
}

async fn initialize_admin_services(config: &AdminConfig) -> Result<AdminServices> {
    run_admin_migrations(&config.database.url).await?;
    let tera = load_templates()?;
    let ruby_status = load_ruby_status().await;

    Ok(AdminServices { tera, ruby_status })
}

fn build_admin_state(runtime: VeinRuntime, services: AdminServices) -> AdminState {
    let resources = AdminResources::new(runtime.config, runtime.cache, services.ruby_status);
    AdminState::new(resources, services.tera)
}

fn spawn_background_jobs(cache: Arc<CacheBackend>) -> Result<()> {
    catalog::spawn_background_sync(cache)
}

fn load_vein_config(config: &AdminConfig) -> Result<Arc<VeinConfig>> {
    let config = Arc::new(
        VeinConfig::load(Some(config.vein.config_path.clone().into())).context("loading config")?,
    );
    config.validate().context("validating config")?;
    Ok(config)
}

async fn prepare_storage(config: &VeinConfig) -> Result<()> {
    FilesystemStorage::new(config.storage.path.clone())
        .prepare()
        .await
        .context("preparing storage")
}

async fn connect_cache(vein_config: &Arc<VeinConfig>) -> Result<Arc<CacheBackend>> {
    let (cache, _) = connect_cache_backend(vein_config)
        .await
        .context("connecting to cache backend")?;
    Ok(cache)
}

async fn run_admin_migrations(database_url: &str) -> Result<()> {
    let db = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await
        .context("connecting admin database")?;
    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .context("running admin migrations")?;
    Ok(())
}

fn load_templates() -> Result<Arc<Tera>> {
    Ok(Arc::new(
        Tera::new(TEMPLATE_GLOB).context("loading admin templates")?,
    ))
}

async fn load_ruby_status() -> Arc<ruby::RubyStatus> {
    match ruby::fetch_ruby_status().await {
        Ok(status) => Arc::new(status),
        Err(err) => {
            warn!(error = %err, "failed to fetch ruby status");
            Arc::new(ruby::RubyStatus::default())
        }
    }
}
