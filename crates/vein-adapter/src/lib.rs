// Unexercised `pub` items are expected in a library compiled as a test binary.
#![cfg_attr(test, allow(dead_code_pub_in_binary))]

mod cache;
mod storage;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
mod version;

// Core types (always available)
pub use cache::{
    AssetKey, AssetKind, CacheBackendTrait, CachedAsset, CachedVersion, CatalogName,
    DependencyKind, ExportMatch, GemDependency, GemMetadata, GemSymbolRecord, IndexStats,
    PackageExport, PendingExports, SbomCoverage,
};

pub use cache::PostgresCacheBackend as CacheBackend;

// Security advisories
pub use cache::{Advisory, AdvisoryImport, Affected, AffectedPackage, Interval};

pub use storage::{FileHandle, FilesystemStorage, TempFile};
pub use version::{compare_gem_versions, is_semver_prerelease, sort_newest_first};

// Quarantine types
pub use cache::{
    DelayPolicy, PackageVersion, QuarantineStats, TrustedPackage, VersionStatus,
    calculate_availability,
};
