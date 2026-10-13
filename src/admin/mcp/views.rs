//! JSON shapes shared by several MCP resources.

use serde::Serialize;
use vein_adapter::{CachedVersion, GemMetadata, GemVersion, QuarantineStats};

/// Result of any `.../search/{query}` resource.
#[derive(Serialize)]
pub struct Search {
    pub query: String,
    /// `rubygems.org`: every published name. `cached`: packages this proxy
    /// holds, so a miss says nothing about whether the package exists.
    pub scope: &'static str,
    /// Exact match first, then prefix matches, then the rest.
    pub names: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog_size: Option<u64>,
}

/// One crate or npm version.
#[derive(Serialize)]
pub struct PackageVersion<D> {
    pub name: String,
    pub version: String,
    /// The package file held here, if downloaded through the proxy.
    pub cached: Option<Artifact>,
    /// What the registry publishes for this version, from its cached index
    /// or metadata.
    pub published: Option<D>,
}

/// A package file held in the proxy's storage.
#[derive(Serialize)]
pub struct Artifact {
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    pub sha256: String,
    pub size_bytes: u64,
    pub last_accessed: String,
}

#[derive(Serialize)]
pub struct VersionDetails {
    pub name: String,
    pub version: String,
    pub platform: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub licenses: Vec<String>,
    pub authors: Vec<String>,
    pub homepage: Option<String>,
    pub source_code_url: Option<String>,
    pub documentation_url: Option<String>,
    pub changelog_url: Option<String>,
    pub required_ruby_version: Option<String>,
    pub dependencies: Vec<Dependency>,
    /// Languages of bundled native extensions, e.g. `c` or `rust`.
    pub native_languages: Vec<String>,
    pub has_native_extensions: bool,
    pub has_embedded_binaries: bool,
    pub size_bytes: u64,
    pub sha256: String,
    /// Where to read the `CycloneDX` SBOM, when one was generated.
    pub sbom: Option<String>,
}

#[derive(Serialize)]
pub struct Dependency {
    pub name: String,
    pub requirement: String,
    pub kind: String,
}

#[derive(Serialize)]
pub struct TrackedVersion {
    pub name: String,
    pub version: String,
    pub platform: Option<String>,
    /// `quarantine`, `available`, `yanked`, or `pinned`.
    pub status: String,
    /// RFC 3339 time the version leaves quarantine.
    pub available_after: String,
    pub status_reason: Option<String>,
    pub upstream_yanked: bool,
}

#[derive(Serialize)]
pub struct QuarantineCounts {
    pub quarantined: u64,
    pub available: u64,
    pub yanked: u64,
    pub pinned: u64,
    pub releasing_today: u64,
    pub releasing_this_week: u64,
}

/// URI of a gem version resource: `version`, plus `-platform` unless `ruby`.
pub fn gem_version_uri(name: &str, version: &str, platform: &str) -> String {
    match platform {
        super::DEFAULT_PLATFORM => format!("vein://gems/{name}/versions/{version}"),
        platform => format!("vein://gems/{name}/versions/{version}-{platform}"),
    }
}

/// URI of a gem version's `CycloneDX` SBOM resource.
pub fn gem_sbom_uri(name: &str, version: &str, platform: &str) -> String {
    format!("{}/sbom", gem_version_uri(name, version, platform))
}

impl From<CachedVersion> for Artifact {
    fn from(cached: CachedVersion) -> Self {
        Self {
            version: cached.version,
            platform: cached.platform,
            sha256: cached.sha256,
            size_bytes: cached.size_bytes,
            last_accessed: cached.last_accessed,
        }
    }
}

impl From<GemMetadata> for VersionDetails {
    fn from(meta: GemMetadata) -> Self {
        let sbom = meta
            .sbom
            .as_ref()
            .map(|_| gem_sbom_uri(&meta.name, &meta.version, &meta.platform));
        Self {
            sbom,
            dependencies: meta
                .dependencies
                .into_iter()
                .map(|dep| Dependency {
                    kind: serde_json::to_value(&dep.kind)
                        .ok()
                        .and_then(|kind| kind.as_str().map(str::to_string))
                        .unwrap_or_else(|| "unknown".to_string()),
                    name: dep.name,
                    requirement: dep.requirement,
                })
                .collect(),
            name: meta.name,
            version: meta.version,
            platform: meta.platform,
            summary: meta.summary,
            description: meta.description,
            licenses: meta.licenses,
            authors: meta.authors,
            homepage: meta.homepage,
            source_code_url: meta.source_code_url,
            documentation_url: meta.documentation_url,
            changelog_url: meta.changelog_url,
            required_ruby_version: meta.required_ruby_version,
            native_languages: meta.native_languages,
            has_native_extensions: meta.has_native_extensions,
            has_embedded_binaries: meta.has_embedded_binaries,
            size_bytes: meta.size_bytes,
            sha256: meta.sha256,
        }
    }
}

impl From<GemVersion> for TrackedVersion {
    fn from(version: GemVersion) -> Self {
        Self {
            status: version.status.to_string(),
            available_after: version.available_after.to_rfc3339(),
            name: version.name,
            version: version.version,
            platform: version.platform,
            status_reason: version.status_reason,
            upstream_yanked: version.upstream_yanked,
        }
    }
}

impl From<QuarantineStats> for QuarantineCounts {
    fn from(stats: QuarantineStats) -> Self {
        Self {
            quarantined: stats.total_quarantined,
            available: stats.total_available,
            yanked: stats.total_yanked,
            pinned: stats.total_pinned,
            releasing_today: stats.versions_releasing_today,
            releasing_this_week: stats.versions_releasing_this_week,
        }
    }
}
