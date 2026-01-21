use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    Gem,
    Spec,
    Crate,
    NpmPackage,
}

impl AssetKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AssetKind::Gem => "gem",
            AssetKind::Spec => "gemspec",
            AssetKind::Crate => "crate",
            AssetKind::NpmPackage => "npm",
        }
    }
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    pub gem_assets: u64,
    pub spec_assets: u64,
    pub unique_gems: u64,
    pub total_size_bytes: u64,
    pub last_accessed: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SbomCoverage {
    pub metadata_rows: u64,
    pub with_sbom: u64,
}
