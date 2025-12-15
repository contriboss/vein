pub use sea_orm_migration::prelude::*;

mod m20250723_000001_create_cached_assets;
mod m20250723_000002_create_catalog_gems;
mod m20250723_000003_create_catalog_meta;
mod m20250723_000004_create_gem_metadata;
mod m20250723_000005_add_sbom_column;
mod m20250723_000006_add_native_languages_column;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250723_000001_create_cached_assets::Migration),
            Box::new(m20250723_000002_create_catalog_gems::Migration),
            Box::new(m20250723_000003_create_catalog_meta::Migration),
            Box::new(m20250723_000004_create_gem_metadata::Migration),
            Box::new(m20250723_000005_add_sbom_column::Migration),
            Box::new(m20250723_000006_add_native_languages_column::Migration),
        ]
    }
}
