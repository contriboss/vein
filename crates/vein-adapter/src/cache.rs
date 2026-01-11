pub mod models;
#[cfg(feature = "postgres")]
pub mod postgres;
pub mod quarantine;
pub mod serialization;
#[cfg(feature = "sqlite")]
pub mod sqlite;
#[cfg(test)]
mod tests;
pub mod types;

use std::future::Future;

use anyhow::Result;
use chrono::{DateTime, Utc};

// Re-export commonly used types
pub use types::{
    AssetKey, AssetKind, CachedAsset, DependencyKind, GemDependency, GemMetadata, IndexStats,
    SbomCoverage,
};

// Re-export quarantine types
pub use quarantine::{
    calculate_availability, is_version_available, is_version_downloadable, DelayPolicy,
    GemVersion, QuarantineInfo, QuarantineStats, VersionStatus,
};

// Re-export backend implementations (conditional)
#[cfg(feature = "postgres")]
pub use postgres::PostgresCacheBackend;
#[cfg(feature = "sqlite")]
pub use sqlite::SqliteCacheBackend;

// Public trait for cache backend implementations
// Uses explicit `impl Future + Send` to satisfy auto-trait bounds without #[allow]
pub trait CacheBackendTrait: Send + Sync {
    fn get(&self, key: &AssetKey<'_>) -> impl Future<Output = Result<Option<CachedAsset>>> + Send;
    fn insert_or_replace(
        &self,
        key: &AssetKey<'_>,
        path: &str,
        sha256: &str,
        size_bytes: u64,
    ) -> impl Future<Output = Result<()>> + Send;
    fn get_all_gems(&self) -> impl Future<Output = Result<Vec<(String, String)>>> + Send;
    fn stats(&self) -> impl Future<Output = Result<IndexStats>> + Send;
    fn catalog_upsert_names(&self, names: &[String]) -> impl Future<Output = Result<()>> + Send;
    fn catalog_total(&self) -> impl Future<Output = Result<u64>> + Send;
    fn catalog_page(&self, offset: i64, limit: i64)
        -> impl Future<Output = Result<Vec<String>>> + Send;
    fn catalog_search(&self, query: &str, limit: i64)
        -> impl Future<Output = Result<Vec<String>>> + Send;
    fn catalog_meta_get(&self, key: &str)
        -> impl Future<Output = Result<Option<String>>> + Send;
    fn catalog_meta_set(&self, key: &str, value: &str) -> impl Future<Output = Result<()>> + Send;
    fn upsert_metadata(&self, metadata: &GemMetadata) -> impl Future<Output = Result<()>> + Send;
    fn gem_metadata(
        &self,
        name: &str,
        version: &str,
        platform: Option<&str>,
    ) -> impl Future<Output = Result<Option<GemMetadata>>> + Send;
    fn sbom_coverage(&self) -> impl Future<Output = Result<SbomCoverage>> + Send;
    fn catalog_languages(&self) -> impl Future<Output = Result<Vec<String>>> + Send;
    fn catalog_page_by_language(
        &self,
        language: &str,
        offset: i64,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<String>>> + Send;
    fn catalog_total_by_language(&self, language: &str)
        -> impl Future<Output = Result<u64>> + Send;

    // ==================== Quarantine Methods ====================

    fn get_gem_version(
        &self,
        name: &str,
        version: &str,
        platform: Option<&str>,
    ) -> impl Future<Output = Result<Option<GemVersion>>> + Send;

    fn upsert_gem_version(
        &self,
        gem_version: &GemVersion,
    ) -> impl Future<Output = Result<()>> + Send;

    fn get_latest_available_version(
        &self,
        name: &str,
        platform: Option<&str>,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<GemVersion>>> + Send;

    fn get_quarantined_versions(
        &self,
        name: &str,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<GemVersion>>> + Send;

    fn update_version_status(
        &self,
        name: &str,
        version: &str,
        platform: Option<&str>,
        status: VersionStatus,
        reason: Option<String>,
    ) -> impl Future<Output = Result<()>> + Send;

    fn promote_expired_quarantines(
        &self,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<u64>> + Send;

    fn mark_yanked(&self, name: &str, version: &str) -> impl Future<Output = Result<()>> + Send;

    fn get_all_quarantined(
        &self,
        limit: u32,
        offset: u32,
    ) -> impl Future<Output = Result<Vec<GemVersion>>> + Send;

    fn quarantine_stats(&self) -> impl Future<Output = Result<QuarantineStats>> + Send;

    fn get_gem_versions_for_index(
        &self,
        name: &str,
    ) -> impl Future<Output = Result<Vec<GemVersion>>> + Send;

    // ==================== Symbol Indexing Methods ====================

    fn insert_symbols(
        &self,
        gem_name: &str,
        gem_version: &str,
        gem_platform: Option<&str>,
        file_path: &str,
        symbol_type: &str,
        symbol_name: &str,
        parent_name: Option<&str>,
        line_number: Option<i32>,
    ) -> impl Future<Output = Result<()>> + Send;

    fn clear_symbols(
        &self,
        gem_name: &str,
        gem_version: &str,
        gem_platform: Option<&str>,
    ) -> impl Future<Output = Result<()>> + Send;

    // ==================== Admin Settings Methods ====================

    fn admin_setting_get(&self, key: &str) -> impl Future<Output = Result<Option<String>>> + Send;

    fn admin_setting_set(&self, key: &str, value: &str) -> impl Future<Output = Result<()>> + Send;
}
