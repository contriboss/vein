use jiff::Timestamp;
use sqlx::FromRow;

use super::quarantine::PackageVersion;
use super::types::{CachedAsset, CachedVersion};

#[derive(Debug, FromRow)]
pub struct CachedAssetRow {
    pub path: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub last_accessed: jiff_sqlx::Timestamp,
}

impl From<CachedAssetRow> for CachedAsset {
    fn from(value: CachedAssetRow) -> Self {
        Self {
            path: value.path,
            sha256: value.sha256,
            size_bytes: value.size_bytes.max(0).cast_unsigned(),
            last_accessed: format_timestamp(value.last_accessed.to_jiff()),
        }
    }
}

#[derive(Debug, FromRow)]
pub struct CachedVersionRow {
    pub version: String,
    pub platform: Option<String>,
    pub sha256: String,
    pub size_bytes: i64,
    pub last_accessed: jiff_sqlx::Timestamp,
}

impl From<CachedVersionRow> for CachedVersion {
    fn from(value: CachedVersionRow) -> Self {
        Self {
            version: value.version,
            platform: value.platform,
            sha256: value.sha256,
            size_bytes: value.size_bytes.max(0).cast_unsigned(),
            last_accessed: format_timestamp(value.last_accessed.to_jiff()),
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

/// RFC 3339 in UTC with milliseconds: `2026-10-23T09:00:00.000Z`.
pub fn format_timestamp(ts: Timestamp) -> String {
    format!("{ts:.3}")
}

/// Row of the `package_versions` table.
#[derive(Debug, FromRow)]
pub struct PackageVersionRow {
    pub id: i64,
    pub kind: String,
    pub name: String,
    pub version: String,
    pub platform: Option<String>,
    pub published_at: jiff_sqlx::Timestamp,
    pub available_after: jiff_sqlx::Timestamp,
    pub status: String,
    pub status_reason: Option<String>,
    pub created_at: jiff_sqlx::Timestamp,
    pub updated_at: jiff_sqlx::Timestamp,
}

impl TryFrom<PackageVersionRow> for PackageVersion {
    type Error = anyhow::Error;

    fn try_from(row: PackageVersionRow) -> anyhow::Result<Self> {
        Ok(Self {
            id: row.id,
            kind: row.kind.parse().map_err(anyhow::Error::msg)?,
            name: row.name,
            version: row.version,
            platform: row.platform,
            published_at: row.published_at.to_jiff(),
            available_after: row.available_after.to_jiff(),
            status: row
                .status
                .parse()
                .map_err(|()| anyhow::anyhow!("unknown version status `{}`", row.status))?,
            status_reason: row.status_reason,
            created_at: row.created_at.to_jiff(),
            updated_at: row.updated_at.to_jiff(),
        })
    }
}
