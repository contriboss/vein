use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::FromRow;

use super::quarantine::GemVersion;
use super::types::{CachedAsset, CachedVersion};

#[derive(Debug, FromRow)]
pub struct CachedAssetRow {
    pub path: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub last_accessed: DateTime<Utc>,
}

impl From<CachedAssetRow> for CachedAsset {
    fn from(value: CachedAssetRow) -> Self {
        Self {
            path: value.path,
            sha256: value.sha256,
            size_bytes: value.size_bytes.max(0).cast_unsigned(),
            last_accessed: format_timestamp(value.last_accessed),
        }
    }
}

#[derive(Debug, FromRow)]
pub struct CachedVersionRow {
    pub version: String,
    pub platform: Option<String>,
    pub sha256: String,
    pub size_bytes: i64,
    pub last_accessed: DateTime<Utc>,
}

impl From<CachedVersionRow> for CachedVersion {
    fn from(value: CachedVersionRow) -> Self {
        Self {
            version: value.version,
            platform: value.platform,
            sha256: value.sha256,
            size_bytes: value.size_bytes.max(0).cast_unsigned(),
            last_accessed: format_timestamp(value.last_accessed),
        }
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct DbGemMetadataRow {
    pub name: String,
    pub version: String,
    pub platform: String,
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

/// Row of the `gem_versions` table.
#[derive(Debug, FromRow)]
pub struct GemVersionRow {
    pub id: i64,
    pub name: String,
    pub version: String,
    pub platform: Option<String>,
    pub sha256: Option<String>,
    pub published_at: DateTime<Utc>,
    pub available_after: DateTime<Utc>,
    pub status: String,
    pub status_reason: Option<String>,
    pub upstream_yanked: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<GemVersionRow> for GemVersion {
    fn from(row: GemVersionRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            version: row.version,
            platform: row.platform,
            sha256: row.sha256,
            published_at: row.published_at,
            available_after: row.available_after,
            status: row.status.parse().unwrap_or_default(),
            status_reason: row.status_reason,
            upstream_yanked: row.upstream_yanked,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}
