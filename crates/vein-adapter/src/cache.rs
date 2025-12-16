pub mod models;
pub mod postgres;
pub mod quarantine;
pub mod serialization;
pub mod sqlite;
#[cfg(test)]
mod tests;
pub mod types;

use anyhow::Result;
use async_trait::async_trait;
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

    // ==================== Quarantine Methods ====================

    /// Get a specific gem version's quarantine status.
    async fn get_gem_version(
        &self,
        name: &str,
        version: &str,
        platform: Option<&str>,
    ) -> Result<Option<GemVersion>>;

    /// Insert or update a gem version's quarantine record.
    async fn upsert_gem_version(&self, gem_version: &GemVersion) -> Result<()>;

    /// Get the latest available (non-quarantined) version of a gem.
    async fn get_latest_available_version(
        &self,
        name: &str,
        platform: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<Option<GemVersion>>;

    /// Get all currently quarantined versions for a gem.
    async fn get_quarantined_versions(
        &self,
        name: &str,
        now: DateTime<Utc>,
    ) -> Result<Vec<GemVersion>>;

    /// Update a version's status (e.g., pin for emergency release).
    async fn update_version_status(
        &self,
        name: &str,
        version: &str,
        platform: Option<&str>,
        status: VersionStatus,
        reason: Option<String>,
    ) -> Result<()>;

    /// Promote all quarantined versions whose delay has expired.
    /// Returns the number of versions promoted.
    async fn promote_expired_quarantines(&self, now: DateTime<Utc>) -> Result<u64>;

    /// Mark a version as yanked (upstream removed it).
    async fn mark_yanked(&self, name: &str, version: &str) -> Result<()>;

    /// Get all quarantined versions (paginated).
    async fn get_all_quarantined(&self, limit: u32, offset: u32) -> Result<Vec<GemVersion>>;

    /// Get quarantine statistics.
    async fn quarantine_stats(&self) -> Result<QuarantineStats>;

    /// Get all versions for a gem (for compact index generation).
    async fn get_gem_versions_for_index(&self, name: &str) -> Result<Vec<GemVersion>>;

    /// Check if quarantine table exists (for migrations).
    async fn quarantine_table_exists(&self) -> Result<bool>;

    /// Run quarantine migrations.
    async fn run_quarantine_migrations(&self) -> Result<()>;
}
