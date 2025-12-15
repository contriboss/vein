use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::FromRow;

use super::types::CachedAsset;

#[derive(Debug, FromRow)]
pub struct CachedAssetRow {
    pub path: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub last_accessed: String,
}

#[derive(Debug, FromRow)]
pub struct PostgresCachedAssetRow {
    pub path: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub last_accessed: DateTime<Utc>,
}

impl From<CachedAssetRow> for CachedAsset {
    fn from(value: CachedAssetRow) -> Self {
        CachedAsset {
            path: value.path,
            sha256: value.sha256,
            size_bytes: value.size_bytes.max(0) as u64,
            last_accessed: value.last_accessed,
        }
    }
}

impl From<PostgresCachedAssetRow> for CachedAsset {
    fn from(value: PostgresCachedAssetRow) -> Self {
        CachedAsset {
            path: value.path,
            sha256: value.sha256,
            size_bytes: value.size_bytes.max(0) as u64,
            last_accessed: format_timestamp(value.last_accessed),
        }
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct DbGemMetadataRow {
    pub name: String,
    pub version: String,
    pub platform: Option<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub licenses: String,
    pub authors: String,
    pub emails: String,
    pub homepage: Option<String>,
    pub documentation_url: Option<String>,
    pub changelog_url: Option<String>,
    pub source_code_url: Option<String>,
    pub bug_tracker_url: Option<String>,
    pub wiki_url: Option<String>,
    pub funding_url: Option<String>,
    pub metadata_json: Option<String>,
    pub dependencies_json: String,
    pub executables_json: Option<String>,
    pub extensions_json: Option<String>,
    pub native_languages_json: Option<String>,
    pub has_native_extensions: bool,
    pub has_embedded_binaries: bool,
    pub required_ruby_version: Option<String>,
    pub required_rubygems_version: Option<String>,
    pub rubygems_version: Option<String>,
    pub specification_version: Option<i64>,
    pub built_at: Option<String>,
    pub size_bytes: i64,
    pub sha256: String,
    pub sbom_json: Option<String>,
}

pub fn format_timestamp(ts: DateTime<Utc>) -> String {
    ts.to_rfc3339_opts(SecondsFormat::Millis, true)
}
