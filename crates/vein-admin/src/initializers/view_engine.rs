use async_trait::async_trait;
use loco_rs::{
    app::{AppContext, Initializer},
    prelude::*,
};
use std::sync::Arc;
use tera::Tera;

pub struct ViewEngineInitializer;

#[async_trait]
impl Initializer for ViewEngineInitializer {
    fn name(&self) -> String {
        "view-engine".to_string()
    }

    async fn after_routes(&self, router: axum::Router, ctx: &AppContext) -> Result<axum::Router> {
        // Initialize Tera templates
        let tera = Arc::new(
            Tera::new("crates/vein-admin/assets/views/**/*.html")
                .map_err(|e| Error::Message(format!("Failed to load templates: {}", e)))?
        );

        tracing::info!("Loaded {} templates", tera.get_template_names().count());

        // Store Tera in the shared context
        ctx.shared_store.insert(tera);

        Ok(router)
    }
}
