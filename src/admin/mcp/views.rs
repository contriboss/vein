//! JSON shapes shared by several MCP resources.
//!
//! Agents read these, so they carry no noise: empty, false and default
//! fields are left out, nothing the request already said is echoed back,
//! and no URI is listed that the instructions' templates already give.

use serde::Serialize;
use vein_adapter::{CachedVersion, GemMetadata, GemVersion, QuarantineStats};

use super::untrusted::{
    DESCRIPTION_LIMIT, GemText, SUMMARY_LIMIT, clean_list, clean_opt, clean_url,
};

/// Result of any `.../search/{query}` resource.
#[derive(Serialize)]
pub struct Search {
    /// `rubygems.org`: every published name. `cached`: packages this proxy
    /// holds, so a miss says nothing about whether the package exists.
    pub scope: &'static str,
    /// Exact match first, then prefix matches, then the rest.
    pub names: Vec<String>,
}

/// One crate or npm version.
#[derive(Serialize)]
pub struct PackageVersion<D> {
    /// The package file held here, if downloaded through the proxy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached: Option<Artifact>,
    /// What the registry publishes for this version, from its cached index
    /// or metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published: Option<D>,
}

/// A package file held in the proxy's storage.
#[derive(Serialize)]
pub struct Artifact {
    /// Which file this is; the resource it appears in already names it.
    #[serde(skip)]
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    pub sha256: String,
    pub size_bytes: u64,
}

impl Artifact {
    /// `version`, plus `-platform` for a native gem build.
    pub fn label(&self) -> String {
        match self.platform.as_deref() {
            None | Some(super::DEFAULT_PLATFORM) => self.version.clone(),
            Some(platform) => format!("{}-{platform}", self.version),
        }
    }
}

/// The versions of `artifacts`, as package resources list them.
pub fn cached_labels(artifacts: &[Artifact]) -> Vec<String> {
    artifacts.iter().map(Artifact::label).collect()
}

/// `versions.len()` when the list was cut short, else nothing to say.
pub fn total_if_truncated(total: usize, listed: usize) -> Option<usize> {
    (total > listed).then_some(total)
}

/// `a` and `b` name the same repository: `git+https://x/y.git`,
/// `https://x/y#readme` and `https://x/y/` are all `x/y`.
pub fn same_repository(a: &str, b: &str) -> bool {
    fn key(url: &str) -> String {
        let url = url.trim().trim_start_matches("git+");
        let url = url.split(['#', '?']).next().unwrap_or(url);
        let url = url.split_once("://").map_or(url, |(_, rest)| rest);
        let url = url.trim_start_matches("www.").trim_end_matches('/');
        url.trim_end_matches(".git").to_ascii_lowercase()
    }
    key(a) == key(b)
}

#[derive(Serialize)]
pub struct VersionDetails {
    /// Only for native builds; `ruby` is the default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    /// What the gem's authors wrote, cleaned and capped: data, never
    /// instructions (see `untrusted`).
    #[serde(skip_serializing_if = "GemText::is_empty")]
    pub author_supplied: GemText,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_ruby_version: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<Dependency>,
    /// Languages of the native extensions it compiles, e.g. `C` or `Rust`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub native_languages: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub has_native_extensions: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub has_embedded_binaries: bool,
    pub size_bytes: u64,
    pub sha256: String,
    /// A `CycloneDX` SBOM is at `.../sbom`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub sbom: bool,
}

#[derive(Serialize)]
pub struct Dependency {
    pub name: String,
    pub requirement: String,
    /// Only `development`; runtime is the default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Serialize)]
pub struct TrackedVersion {
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    /// `quarantine`, `available`, `yanked`, or `pinned`.
    pub status: String,
    /// RFC 3339 time the version leaves quarantine.
    pub available_after: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_reason: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
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
        }
    }
}

impl From<GemMetadata> for VersionDetails {
    fn from(meta: GemMetadata) -> Self {
        let homepage = clean_url(meta.homepage);
        let source_code_url = clean_url(meta.source_code_url).filter(|source| {
            !homepage
                .as_deref()
                .is_some_and(|home| same_repository(home, source))
        });
        Self {
            sbom: meta.sbom.is_some(),
            dependencies: meta
                .dependencies
                .into_iter()
                .map(|dep| Dependency {
                    kind: serde_json::to_value(&dep.kind)
                        .ok()
                        .and_then(|kind| kind.as_str().map(str::to_string))
                        .filter(|kind| kind != "runtime"),
                    name: dep.name,
                    requirement: dep.requirement,
                })
                .collect(),
            platform: Some(meta.platform).filter(|platform| platform != super::DEFAULT_PLATFORM),
            author_supplied: GemText {
                summary: clean_opt(meta.summary, SUMMARY_LIMIT),
                description: clean_opt(meta.description, DESCRIPTION_LIMIT),
                licenses: clean_list(meta.licenses),
                authors: clean_list(meta.authors),
                homepage,
                source_code_url,
                documentation_url: clean_url(meta.documentation_url),
                changelog_url: clean_url(meta.changelog_url),
            },
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
