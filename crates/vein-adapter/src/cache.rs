pub mod models;
#[cfg(feature = "postgres")]
pub mod postgres;
pub mod quarantine;
pub mod serialization;
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

// Re-export backend implementations
#[cfg(feature = "postgres")]
pub use postgres::PostgresCacheBackend;
pub use sqlite::SqliteCacheBackend;

/// Enum dispatch for cache backends - zero-cost abstraction over SQLite/Postgres
#[derive(Debug, Clone)]
pub enum CacheBackendKind {
    Sqlite(SqliteCacheBackend),
    #[cfg(feature = "postgres")]
    Postgres(PostgresCacheBackend),
}

/// Macro to delegate async methods to the inner backend
macro_rules! delegate {
    ($self:ident, $method:ident $(, $arg:expr)*) => {
        match $self {
            CacheBackendKind::Sqlite(b) => b.$method($($arg),*).await,
            #[cfg(feature = "postgres")]
            CacheBackendKind::Postgres(b) => b.$method($($arg),*).await,
        }
    };
}

impl CacheBackendKind {
    pub async fn get(&self, key: &AssetKey<'_>) -> Result<Option<CachedAsset>> {
        delegate!(self, get, key)
    }

    pub async fn insert_or_replace(
        &self,
        key: &AssetKey<'_>,
        path: &str,
        sha256: &str,
        size_bytes: u64,
    ) -> Result<()> {
        delegate!(self, insert_or_replace, key, path, sha256, size_bytes)
    }

    pub async fn get_all_gems(&self) -> Result<Vec<(String, String)>> {
        delegate!(self, get_all_gems)
    }

    pub async fn stats(&self) -> Result<IndexStats> {
        delegate!(self, stats)
    }

    pub async fn catalog_upsert_names(&self, names: &[String]) -> Result<()> {
        delegate!(self, catalog_upsert_names, names)
    }

    pub async fn catalog_total(&self) -> Result<u64> {
        delegate!(self, catalog_total)
    }

    pub async fn catalog_page(&self, offset: i64, limit: i64) -> Result<Vec<String>> {
        delegate!(self, catalog_page, offset, limit)
    }

    pub async fn catalog_meta_get(&self, key: &str) -> Result<Option<String>> {
        delegate!(self, catalog_meta_get, key)
    }

    pub async fn catalog_meta_set(&self, key: &str, value: &str) -> Result<()> {
        delegate!(self, catalog_meta_set, key, value)
    }

    pub async fn upsert_metadata(&self, metadata: &GemMetadata) -> Result<()> {
        delegate!(self, upsert_metadata, metadata)
    }

    pub async fn gem_metadata(
        &self,
        name: &str,
        version: &str,
        platform: Option<&str>,
    ) -> Result<Option<GemMetadata>> {
        delegate!(self, gem_metadata, name, version, platform)
    }

    pub async fn sbom_coverage(&self) -> Result<SbomCoverage> {
        delegate!(self, sbom_coverage)
    }

    pub async fn catalog_languages(&self) -> Result<Vec<String>> {
        delegate!(self, catalog_languages)
    }

    pub async fn catalog_page_by_language(
        &self,
        language: &str,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<String>> {
        delegate!(self, catalog_page_by_language, language, offset, limit)
    }

    pub async fn catalog_total_by_language(&self, language: &str) -> Result<u64> {
        delegate!(self, catalog_total_by_language, language)
    }

    pub async fn get_gem_version(
        &self,
        name: &str,
        version: &str,
        platform: Option<&str>,
    ) -> Result<Option<GemVersion>> {
        delegate!(self, get_gem_version, name, version, platform)
    }

    pub async fn upsert_gem_version(&self, gem_version: &GemVersion) -> Result<()> {
        delegate!(self, upsert_gem_version, gem_version)
    }

    pub async fn get_latest_available_version(
        &self,
        name: &str,
        platform: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<Option<GemVersion>> {
        delegate!(self, get_latest_available_version, name, platform, now)
    }

    pub async fn get_quarantined_versions(
        &self,
        name: &str,
        now: DateTime<Utc>,
    ) -> Result<Vec<GemVersion>> {
        delegate!(self, get_quarantined_versions, name, now)
    }

    pub async fn update_version_status(
        &self,
        name: &str,
        version: &str,
        platform: Option<&str>,
        status: VersionStatus,
        reason: Option<String>,
    ) -> Result<()> {
        delegate!(self, update_version_status, name, version, platform, status, reason)
    }

    pub async fn promote_expired_quarantines(&self, now: DateTime<Utc>) -> Result<u64> {
        delegate!(self, promote_expired_quarantines, now)
    }

    pub async fn mark_yanked(&self, name: &str, version: &str) -> Result<()> {
        delegate!(self, mark_yanked, name, version)
    }

    pub async fn get_all_quarantined(&self, limit: u32, offset: u32) -> Result<Vec<GemVersion>> {
        delegate!(self, get_all_quarantined, limit, offset)
    }

    pub async fn quarantine_stats(&self) -> Result<QuarantineStats> {
        delegate!(self, quarantine_stats)
    }

    pub async fn get_gem_versions_for_index(&self, name: &str) -> Result<Vec<GemVersion>> {
        delegate!(self, get_gem_versions_for_index, name)
    }

    pub async fn quarantine_table_exists(&self) -> Result<bool> {
        delegate!(self, quarantine_table_exists)
    }

    pub async fn run_quarantine_migrations(&self) -> Result<()> {
        delegate!(self, run_quarantine_migrations)
    }

    pub async fn run_symbols_migrations(&self) -> Result<()> {
        delegate!(self, run_symbols_migrations)
    }

    pub async fn insert_symbols(
        &self,
        gem_name: &str,
        gem_version: &str,
        gem_platform: Option<&str>,
        file_path: &str,
        symbol_type: &str,
        symbol_name: &str,
        parent_name: Option<&str>,
        line_number: Option<i32>,
    ) -> Result<()> {
        delegate!(
            self,
            insert_symbols,
            gem_name,
            gem_version,
            gem_platform,
            file_path,
            symbol_type,
            symbol_name,
            parent_name,
            line_number
        )
    }

    pub async fn clear_symbols(
        &self,
        gem_name: &str,
        gem_version: &str,
        gem_platform: Option<&str>,
    ) -> Result<()> {
        delegate!(self, clear_symbols, gem_name, gem_version, gem_platform)
    }
}

#[cfg(feature = "postgres")]
impl From<PostgresCacheBackend> for CacheBackendKind {
    fn from(backend: PostgresCacheBackend) -> Self {
        CacheBackendKind::Postgres(backend)
    }
}

impl From<SqliteCacheBackend> for CacheBackendKind {
    fn from(backend: SqliteCacheBackend) -> Self {
        CacheBackendKind::Sqlite(backend)
    }
}

// Internal trait for concrete implementations - public API uses CacheBackendKind enum
// Uses explicit `impl Future + Send` to satisfy auto-trait bounds without #[allow]
pub(crate) trait CacheBackend: Send + Sync {
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

    fn quarantine_table_exists(&self) -> impl Future<Output = Result<bool>> + Send;

    fn run_quarantine_migrations(&self) -> impl Future<Output = Result<()>> + Send;

    // ==================== Symbol Indexing Methods ====================

    fn run_symbols_migrations(&self) -> impl Future<Output = Result<()>> + Send;

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
}
