use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use loco_rs::{
    Result,
    app::{AppContext, Hooks},
    boot::{BootResult, StartMode, create_app},
    controller::AppRoutes,
    environment::Environment,
    prelude::*,
    task::Tasks,
};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{controllers, initializers, ruby, state::AdminResources};
use vein::{catalog, config::Config as VeinConfig, db::connect_cache_backend};
use vein_adapter::FilesystemStorage;

static ADMIN_RESOURCES: OnceLock<AdminResources> = OnceLock::new();

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Settings {
    pub vein_config_path: String,
}

pub struct App;

#[async_trait]
impl Hooks for App {
    fn app_name() -> &'static str {
        env!("CARGO_CRATE_NAME")
    }

    fn app_version() -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    async fn boot(
        mode: StartMode,
        environment: &Environment,
        config: loco_rs::config::Config,
    ) -> Result<BootResult> {
        create_app::<Self, migration::Migrator>(mode, environment, config).await
    }

    fn routes(_ctx: &AppContext) -> AppRoutes {
        AppRoutes::with_default_routes()
            .add_route(controllers::datastar::routes())
            .add_route(controllers::dashboard::routes())
            .add_route(controllers::health::routes())
            .add_route(controllers::catalog::routes())
            .add_route(controllers::changelog::routes())
            .add_route(controllers::permissions::routes())
            .add_route(controllers::quarantine::routes())
            .add_route(controllers::security::routes())
    }

    async fn after_context(ctx: AppContext) -> Result<AppContext> {
        // Initialize Vein resources
        let settings = ctx
            .config
            .settings
            .clone()
            .and_then(|s| serde_json::from_value::<Settings>(s).ok())
            .unwrap_or_else(|| Settings {
                vein_config_path: "vein.toml".to_string(),
            });

        let vein_config = Arc::new(
            VeinConfig::load(Some(settings.vein_config_path.into()))
                .map_err(|e| Error::Message(format!("Failed to load vein config: {}", e)))?,
        );

        vein_config
            .validate()
            .map_err(|e| Error::Message(format!("Invalid vein configuration: {}", e)))?;

        // Prepare storage
        FilesystemStorage::new(vein_config.storage.path.clone())
            .prepare()
            .await
            .ok();

        // Connect to cache backend
        let (cache_backend, _backend_kind) = connect_cache_backend(vein_config.as_ref())
            .await
            .map_err(|e| Error::Message(format!("Failed to connect to cache: {}", e)))?;

        // Fetch Ruby status
        let ruby_status = match ruby::fetch_ruby_status().await {
            Ok(status) => Arc::new(status),
            Err(err) => {
                warn!(error = %err, "failed to fetch ruby status");
                Arc::new(ruby::RubyStatus::default())
            }
        };

        // Create admin resources
        let resources =
            AdminResources::new(vein_config.clone(), cache_backend.clone(), ruby_status);

        ADMIN_RESOURCES
            .set(resources.clone())
            .map_err(|_| Error::Message("Admin resources already initialized".to_string()))?;

        // Start background catalog sync
        catalog::spawn_background_sync(cache_backend.clone())
            .map_err(|e| Error::Message(format!("Failed to spawn background sync: {}", e)))?;

        // Add resources to shared context
        ctx.shared_store.insert(resources);

        Ok(ctx)
    }

    async fn connect_workers(_ctx: &AppContext, _queue: &Queue) -> Result<()> {
        Ok(())
    }

    fn register_tasks(_tasks: &mut Tasks) {}

    async fn initializers(_ctx: &AppContext) -> Result<Vec<Box<dyn loco_rs::app::Initializer>>> {
        Ok(vec![Box::new(
            initializers::view_engine::ViewEngineInitializer,
        )])
    }

    async fn truncate(_ctx: &AppContext) -> Result<()> {
        // Truncate tables in test mode if needed
        Ok(())
    }

    async fn seed(_ctx: &AppContext, _base: &std::path::Path) -> Result<()> {
        // Seed data if needed
        Ok(())
    }
}
