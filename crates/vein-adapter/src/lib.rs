// Compile-time checks
#[cfg(not(any(feature = "sqlite", feature = "postgres")))]
compile_error!("Either 'sqlite' or 'postgres' feature must be enabled.");

mod cache;
mod storage;

// Core types (always available)
pub use cache::{
    AssetKey, AssetKind, CachedAsset, CacheBackendTrait, DependencyKind, GemDependency,
    GemMetadata, IndexStats, SbomCoverage,
};

// Export both backends when both features are enabled (for testing)
#[cfg(all(feature = "sqlite", feature = "postgres"))]
pub use cache::{PostgresCacheBackend, SqliteCacheBackend};

// Backend type alias - compile-time selection for single feature
#[cfg(all(feature = "sqlite", not(feature = "postgres")))]
pub use cache::SqliteCacheBackend as CacheBackend;

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
pub use cache::PostgresCacheBackend as CacheBackend;

// Default to SQLite when both are enabled
#[cfg(all(feature = "sqlite", feature = "postgres"))]
pub use cache::SqliteCacheBackend as CacheBackend;

pub use storage::{FileHandle, FilesystemStorage, TempFile};

// Quarantine types
pub use cache::{
    calculate_availability, is_version_available, is_version_downloadable, DelayPolicy,
    GemVersion, QuarantineInfo, QuarantineStats, VersionStatus,
};
