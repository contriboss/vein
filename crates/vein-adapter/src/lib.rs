mod cache;
mod storage;

pub use cache::{
    AssetKey, AssetKind, CacheBackendKind, CachedAsset, DependencyKind, GemDependency,
    GemMetadata, IndexStats, SbomCoverage, SqliteCacheBackend,
};
#[cfg(feature = "postgres")]
pub use cache::PostgresCacheBackend;
pub use storage::{FileHandle, FilesystemStorage, TempFile};

// Quarantine types
pub use cache::{
    calculate_availability, is_version_available, is_version_downloadable, DelayPolicy,
    GemVersion, QuarantineInfo, QuarantineStats, VersionStatus,
};
