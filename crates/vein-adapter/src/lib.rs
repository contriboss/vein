// Unexercised `pub` items are expected in a library compiled as a test binary.
#![cfg_attr(test, allow(dead_code_pub_in_binary))]

mod cache;
mod storage;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
mod version;

// Core types (always available)
pub use cache::{
    AssetKey, AssetKind, CacheBackendTrait, CachedAsset, CachedVersion, DependencyKind,
    GemDependency, GemMetadata, GemSymbolRecord, IndexStats, SbomCoverage,
};

pub use cache::PostgresCacheBackend as CacheBackend;

pub use storage::{FileHandle, FilesystemStorage, TempFile};
pub use version::{compare_gem_versions, sort_newest_first};

// Quarantine types
pub use cache::{
    DelayPolicy, GemVersion, QuarantineInfo, QuarantineStats, VersionStatus,
    calculate_availability, is_version_available, is_version_downloadable,
};
