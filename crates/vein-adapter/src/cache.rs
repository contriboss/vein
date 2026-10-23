mod backend_common;
pub mod models;
pub mod postgres;
pub mod quarantine;
pub mod serialization;
// Runs against the Postgres named by `VEIN_TEST_DATABASE_URL`.
#[cfg(test)]
mod tests;
pub mod types;

use std::future::Future;

use anyhow::Result;
use jiff::Timestamp;

#[derive(Clone, Copy, Debug)]
pub struct GemSymbolRecord<'a> {
    pub gem_name: &'a str,
    pub gem_version: &'a str,
    pub gem_platform: Option<&'a str>,
    pub file_path: &'a str,
    pub symbol_type: &'a str,
    pub symbol_name: &'a str,
    pub parent_name: Option<&'a str>,
    pub line_number: Option<i32>,
}

// Re-export commonly used types
pub use types::{
    AssetKey, AssetKind, CachedAsset, CachedVersion, CatalogName, DependencyKind, GemDependency,
    GemMetadata, IndexStats, SbomCoverage,
};

// Re-export quarantine types
pub use quarantine::{
    DelayPolicy, PackageVersion, QuarantineStats, VersionStatus, calculate_availability,
};

pub use postgres::PostgresCacheBackend;

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
    /// Cached artifacts of package `name`, newest version first.
    fn cached_versions(
        &self,
        kind: AssetKind,
        name: &str,
    ) -> impl Future<Output = Result<Vec<CachedVersion>>> + Send;
    /// Names of cached `kind` packages containing `query`: exact matches
    /// first, then prefix matches, then the rest.
    fn search_cached(
        &self,
        kind: AssetKind,
        query: &str,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<String>>> + Send;
    fn stats(&self) -> impl Future<Output = Result<IndexStats>> + Send;
    fn catalog_upsert_names(&self, names: &[String]) -> impl Future<Output = Result<()>> + Send;
    fn catalog_total(&self) -> impl Future<Output = Result<u64>> + Send;
    fn catalog_page(
        &self,
        offset: i64,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<String>>> + Send;
    fn catalog_search(
        &self,
        query: &str,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<String>>> + Send;
    /// Names registry `kind` publishes, as synced into the catalog, best
    /// matches first (exact, then case-insensitive, then prefix), each with
    /// its latest version when the sync knew it.
    fn names_search(
        &self,
        kind: AssetKind,
        query: &str,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<CatalogName>>> + Send;
    /// Adds or refreshes catalog names of registry `kind`.
    fn names_upsert(
        &self,
        kind: AssetKind,
        names: &[CatalogName],
    ) -> impl Future<Output = Result<()>> + Send;
    /// Drops names the registry no longer publishes.
    fn names_remove(
        &self,
        kind: AssetKind,
        names: &[String],
    ) -> impl Future<Output = Result<()>> + Send;
    /// How many names of registry `kind` the catalog holds.
    fn names_count(&self, kind: AssetKind) -> impl Future<Output = Result<u64>> + Send;
    fn catalog_meta_get(&self, key: &str) -> impl Future<Output = Result<Option<String>>> + Send;
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

    // ==================== Release delay ====================

    /// Every tracked version of package `name` in registry `kind`, newest
    /// first.
    fn package_versions(
        &self,
        kind: AssetKind,
        name: &str,
    ) -> impl Future<Output = Result<Vec<PackageVersion>>> + Send;

    /// Records versions the delay policy holds back. A version already
    /// decided on (approved, pinned, blocked, released) keeps its status;
    /// a held one takes the new timing.
    fn record_held(&self, versions: &[PackageVersion]) -> impl Future<Output = Result<()>> + Send;

    /// An admin's (or the config's) decision on one version, tracked or not.
    fn set_version_status(
        &self,
        version: &AssetKey<'_>,
        status: VersionStatus,
        reason: Option<String>,
    ) -> impl Future<Output = Result<()>> + Send;

    /// Marks held versions whose delay is over as available; the count.
    fn promote_expired_quarantines(
        &self,
        now: Timestamp,
    ) -> impl Future<Output = Result<u64>> + Send;

    /// Versions still held, soonest released first, across registries.
    fn held_versions(
        &self,
        limit: u32,
        offset: u32,
    ) -> impl Future<Output = Result<Vec<PackageVersion>>> + Send;

    fn quarantine_stats(&self) -> impl Future<Output = Result<QuarantineStats>> + Send;

    // ==================== Symbol Indexing Methods ====================

    fn insert_symbols(
        &self,
        symbol: GemSymbolRecord<'_>,
    ) -> impl Future<Output = Result<()>> + Send;

    fn clear_symbols(
        &self,
        gem_name: &str,
        gem_version: &str,
        gem_platform: Option<&str>,
    ) -> impl Future<Output = Result<()>> + Send;
}
