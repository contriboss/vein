use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// What an asset is; also the registry a tracked version belongs to.
/// Serialized, and stored, as [`AssetKind::as_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AssetKind {
    #[serde(rename = "gem", alias = "gems")]
    Gem,
    #[serde(rename = "gemspec")]
    Spec,
    #[serde(rename = "crate", alias = "crates")]
    Crate,
    #[serde(rename = "npm")]
    NpmPackage,
}

impl AssetKind {
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Gem => "gem",
            Self::Spec => "gemspec",
            Self::Crate => "crate",
            Self::NpmPackage => "npm",
        }
    }
}

impl std::str::FromStr for AssetKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "gem" => Ok(Self::Gem),
            "gemspec" => Ok(Self::Spec),
            "crate" => Ok(Self::Crate),
            "npm" => Ok(Self::NpmPackage),
            other => Err(format!("unknown asset kind `{other}`")),
        }
    }
}

/// A name a registry publishes, as its catalog holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogName {
    pub name: String,
    pub latest_version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AssetKey<'a> {
    pub kind: AssetKind,
    pub name: &'a str,
    pub version: &'a str,
    pub platform: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct CachedAsset {
    pub path: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub last_accessed: String,
}

/// One cached artifact of a package, as listed per package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedVersion {
    pub version: String,
    pub platform: Option<String>,
    pub sha256: String,
    pub size_bytes: u64,
    pub last_accessed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    Runtime,
    Development,
    Optional,
    Unknown,
}

impl std::str::FromStr for DependencyKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "runtime" => Self::Runtime,
            "development" => Self::Development,
            "optional" => Self::Optional,
            _ => Self::Unknown,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GemDependency {
    pub name: String,
    pub requirement: String,
    pub kind: DependencyKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GemMetadata {
    pub name: String,
    pub version: String,
    pub platform: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub licenses: Vec<String>,
    pub authors: Vec<String>,
    pub emails: Vec<String>,
    pub homepage: Option<String>,
    pub documentation_url: Option<String>,
    pub changelog_url: Option<String>,
    pub source_code_url: Option<String>,
    pub bug_tracker_url: Option<String>,
    pub wiki_url: Option<String>,
    pub funding_url: Option<String>,
    #[serde(default)]
    pub metadata: JsonValue,
    pub dependencies: Vec<GemDependency>,
    pub executables: Vec<String>,
    pub extensions: Vec<String>,
    #[serde(default)]
    pub native_languages: Vec<String>,
    pub has_native_extensions: bool,
    pub has_embedded_binaries: bool,
    pub required_ruby_version: Option<String>,
    pub required_rubygems_version: Option<String>,
    pub rubygems_version: Option<String>,
    pub specification_version: Option<i64>,
    pub built_at: Option<String>,
    pub size_bytes: u64,
    pub sha256: String,
    #[serde(default)]
    pub sbom: Option<JsonValue>,
}

#[derive(Debug, Clone)]
pub struct IndexStats {
    pub total_assets: u64,
    pub rubygems_assets: u64,
    pub crate_assets: u64,
    pub npm_assets: u64,
    pub unique_packages: u64,
    pub total_size_bytes: u64,
    pub last_accessed: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SbomCoverage {
    pub metadata_rows: u64,
    pub with_sbom: u64,
}
