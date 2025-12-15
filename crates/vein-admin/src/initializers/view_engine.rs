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
        let tera = match Tera::new("crates/vein-admin/assets/views/**/*.html") {
            Ok(t) => Arc::new(t),
            Err(e) => {
                tracing::warn!("Failed to load templates: {}", e);
                Arc::new(Tera::default())
            }
        };

        // Store Tera in the shared context
        ctx.shared_store.insert(tera);

        Ok(router)
    }
}
