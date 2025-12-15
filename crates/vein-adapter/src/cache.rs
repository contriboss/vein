pub mod models;
pub mod postgres;
pub mod serialization;
pub mod sqlite;
#[cfg(test)]
mod tests;
pub mod types;

use anyhow::Result;
use async_trait::async_trait;

// Re-export commonly used types
pub use types::{
    AssetKey, AssetKind, CachedAsset, DependencyKind, GemDependency, GemMetadata, IndexStats,
    SbomCoverage,
};

// Re-export backend implementations
pub use postgres::PostgresCacheBackend;
pub use sqlite::SqliteCacheBackend;

#[async_trait]
pub trait CacheBackend: Send + Sync {
    async fn get(&self, key: &AssetKey<'_>) -> Result<Option<CachedAsset>>;
    async fn insert_or_replace(
        &self,
        key: &AssetKey<'_>,
        path: &str,
        sha256: &str,
        size_bytes: u64,
    ) -> Result<()>;
    async fn get_all_gems(&self) -> Result<Vec<(String, String)>>;
    async fn stats(&self) -> Result<IndexStats>;
    async fn catalog_upsert_names(&self, names: &[String]) -> Result<()>;
    async fn catalog_total(&self) -> Result<u64>;
    async fn catalog_page(&self, offset: i64, limit: i64) -> Result<Vec<String>>;
    async fn catalog_meta_get(&self, key: &str) -> Result<Option<String>>;
    async fn catalog_meta_set(&self, key: &str, value: &str) -> Result<()>;
    async fn upsert_metadata(&self, metadata: &GemMetadata) -> Result<()>;
    async fn gem_metadata(
        &self,
        name: &str,
        version: &str,
        platform: Option<&str>,
    ) -> Result<Option<GemMetadata>>;
    async fn sbom_coverage(&self) -> Result<SbomCoverage>;
    async fn catalog_languages(&self) -> Result<Vec<String>>;
    async fn catalog_page_by_language(
        &self,
        language: &str,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<String>>;
    async fn catalog_total_by_language(&self, language: &str) -> Result<u64>;
}
