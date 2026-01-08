// Compile-time checks for mutually exclusive features
#[cfg(all(feature = "sqlite", feature = "postgres"))]
compile_error!("Features 'sqlite' and 'postgres' are mutually exclusive. Choose one.");

#[cfg(not(any(feature = "sqlite", feature = "postgres")))]
compile_error!("Either 'sqlite' or 'postgres' feature must be enabled.");

mod cache;
mod storage;

// Core types (always available)
pub use cache::{
    AssetKey, AssetKind, CachedAsset, CacheBackendTrait, DependencyKind, GemDependency,
    GemMetadata, IndexStats, SbomCoverage,
};

// Backend type alias - compile-time selection
#[cfg(feature = "sqlite")]
pub use cache::SqliteCacheBackend as CacheBackend;

#[cfg(feature = "postgres")]
pub use cache::PostgresCacheBackend as CacheBackend;

pub use storage::{FileHandle, FilesystemStorage, TempFile};

// Quarantine types
pub use cache::{
    calculate_availability, is_version_available, is_version_downloadable, DelayPolicy,
    GemVersion, QuarantineInfo, QuarantineStats, VersionStatus,
};
