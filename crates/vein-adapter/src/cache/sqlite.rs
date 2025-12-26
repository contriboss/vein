use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};

use super::{
    CacheBackend, GemVersion, QuarantineStats, VersionStatus,
    models::{CachedAssetRow, DbGemMetadataRow, GemVersionRow},
    serialization::{hydrate_metadata_row, parse_language_rows, prepare_metadata_strings},
    types::{AssetKey, CachedAsset, GemMetadata, IndexStats, SbomCoverage},
};

#[derive(Debug, Clone)]
pub struct SqliteCacheBackend {
    pub(crate) pool: SqlitePool,
}

impl SqliteCacheBackend {
    pub async fn connect(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("creating database directory {}", parent.display()))?;
        }
        let conn_str = format!(
            "sqlite://{}",
            path.to_str().context("database path not UTF-8")?
        );
        let pool = SqlitePoolOptions::new()
            .max_connections(16)
            .connect(&conn_str)
            .await
            .with_context(|| format!("connecting to sqlite database {}", path.display()))?;
        let backend = Self { pool };
        backend.init_schema().await?;
        Ok(backend)
    }

    pub async fn connect_memory() -> Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(16)
            .connect("sqlite::memory:")
            .await
            .context("connecting to in-memory sqlite database")?;
        let backend = Self { pool };
        backend.init_schema().await?;
        Ok(backend)
    }

    async fn init_schema(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS cached_assets (
                kind TEXT NOT NULL,
                name TEXT NOT NULL,
                version TEXT NOT NULL,
                platform TEXT,
                path TEXT NOT NULL,
                sha256 TEXT NOT NULL,
                size_bytes INTEGER NOT NULL,
                last_accessed TIMESTAMP DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
                PRIMARY KEY (kind, name, version, platform)
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .context("creating cached_assets table (sqlite)")?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS catalog_gems (
                name TEXT PRIMARY KEY,
                latest_version TEXT,
                synced_at TIMESTAMP DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .context("creating catalog_gems table (sqlite)")?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS catalog_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .context("creating catalog_meta table (sqlite)")?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS gem_metadata (
                name TEXT NOT NULL,
                version TEXT NOT NULL,
                platform TEXT,
                summary TEXT,
                description TEXT,
                licenses TEXT,
                authors TEXT,
                emails TEXT,
                homepage TEXT,
                documentation_url TEXT,
                changelog_url TEXT,
                source_code_url TEXT,
                bug_tracker_url TEXT,
                wiki_url TEXT,
                funding_url TEXT,
                metadata_json TEXT,
                dependencies_json TEXT NOT NULL,
                executables_json TEXT,
                extensions_json TEXT,
                native_languages_json TEXT,
                has_native_extensions INTEGER NOT NULL,
                has_embedded_binaries INTEGER NOT NULL,
                required_ruby_version TEXT,
                required_rubygems_version TEXT,
                rubygems_version TEXT,
                specification_version INTEGER,
                built_at TEXT,
                size_bytes INTEGER,
                sha256 TEXT,
                sbom_json TEXT,
                PRIMARY KEY (name, version, platform)
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .context("creating gem_metadata table (sqlite)")?;

        Ok(())
    }

    async fn touch(&self, key: &AssetKey<'_>) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE cached_assets
            SET last_accessed = strftime('%Y-%m-%dT%H:%M:%fZ','now')
            WHERE kind = ?1 AND name = ?2 AND version = ?3 AND
                  ((platform IS NULL AND ?4 IS NULL) OR platform = ?4)
            "#,
        )
        .bind(key.kind.as_str())
        .bind(key.name)
        .bind(key.version)
        .bind(key.platform)
        .execute(&self.pool)
        .await
        .context("updating last_accessed")?;
        Ok(())
    }

    pub async fn upsert_gem_metadata_record(&self, metadata: &GemMetadata) -> Result<()> {
        let prepared = prepare_metadata_strings(metadata)?;

        sqlx::query(
            r#"
            INSERT INTO gem_metadata(
                name,
                version,
                platform,
                summary,
                description,
                licenses,
                authors,
                emails,
                homepage,
                documentation_url,
                changelog_url,
                source_code_url,
                bug_tracker_url,
                wiki_url,
                funding_url,
                metadata_json,
                dependencies_json,
                executables_json,
                extensions_json,
                native_languages_json,
                has_native_extensions,
                has_embedded_binaries,
                required_ruby_version,
                required_rubygems_version,
                rubygems_version,
                specification_version,
                built_at,
                size_bytes,
                sha256,
                sbom_json
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20,
                ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30
            )
            ON CONFLICT(name, version, platform)
            DO UPDATE SET
                summary = excluded.summary,
                description = excluded.description,
                licenses = excluded.licenses,
                authors = excluded.authors,
                emails = excluded.emails,
                homepage = excluded.homepage,
                documentation_url = excluded.documentation_url,
                changelog_url = excluded.changelog_url,
                source_code_url = excluded.source_code_url,
                bug_tracker_url = excluded.bug_tracker_url,
                wiki_url = excluded.wiki_url,
                funding_url = excluded.funding_url,
                metadata_json = excluded.metadata_json,
                dependencies_json = excluded.dependencies_json,
                executables_json = excluded.executables_json,
                extensions_json = excluded.extensions_json,
                native_languages_json = excluded.native_languages_json,
                has_native_extensions = excluded.has_native_extensions,
                has_embedded_binaries = excluded.has_embedded_binaries,
                required_ruby_version = excluded.required_ruby_version,
                required_rubygems_version = excluded.required_rubygems_version,
                rubygems_version = excluded.rubygems_version,
                specification_version = excluded.specification_version,
                built_at = excluded.built_at,
                size_bytes = excluded.size_bytes,
                sha256 = excluded.sha256,
                sbom_json = excluded.sbom_json
            "#,
        )
        .bind(&metadata.name)
        .bind(&metadata.version)
        .bind(metadata.platform.as_deref())
        .bind(metadata.summary.as_deref())
        .bind(metadata.description.as_deref())
        .bind(prepared.licenses_json)
        .bind(prepared.authors_json)
        .bind(prepared.emails_json)
        .bind(metadata.homepage.as_deref())
        .bind(metadata.documentation_url.as_deref())
        .bind(metadata.changelog_url.as_deref())
        .bind(metadata.source_code_url.as_deref())
        .bind(metadata.bug_tracker_url.as_deref())
        .bind(metadata.wiki_url.as_deref())
        .bind(metadata.funding_url.as_deref())
        .bind(prepared.metadata_json)
        .bind(prepared.dependencies_json)
        .bind(prepared.executables_json)
        .bind(prepared.extensions_json)
        .bind(prepared.native_languages_json)
        .bind(metadata.has_native_extensions)
        .bind(metadata.has_embedded_binaries)
        .bind(metadata.required_ruby_version.as_deref())
        .bind(metadata.required_rubygems_version.as_deref())
        .bind(metadata.rubygems_version.as_deref())
        .bind(metadata.specification_version)
        .bind(metadata.built_at.as_deref())
        .bind(prepared.size_bytes)
        .bind(&metadata.sha256)
        .bind(prepared.sbom_json)
        .execute(&self.pool)
        .await
        .context("upserting gem metadata")?;

        Ok(())
    }

    pub async fn fetch_gem_metadata(
        &self,
        name: &str,
        version: &str,
        platform: Option<&str>,
    ) -> Result<Option<GemMetadata>> {
        let record = sqlx::query_as::<_, DbGemMetadataRow>(
            r#"
            SELECT
                name,
                version,
                platform,
                summary,
                description,
                licenses,
                authors,
                emails,
                homepage,
                documentation_url,
                changelog_url,
                source_code_url,
                bug_tracker_url,
                wiki_url,
                funding_url,
                metadata_json,
                dependencies_json,
                executables_json,
                extensions_json,
                native_languages_json,
                has_native_extensions,
                has_embedded_binaries,
                required_ruby_version,
                required_rubygems_version,
                rubygems_version,
                specification_version,
                built_at,
                size_bytes,
                sha256,
                sbom_json
            FROM gem_metadata
            WHERE name = ?1
              AND version = ?2
              AND ((platform IS NULL AND ?3 IS NULL) OR platform = ?3)
            "#,
        )
        .bind(name)
        .bind(version)
        .bind(platform)
        .fetch_optional(&self.pool)
        .await
        .context("fetching gem metadata record")?;

        match record {
            Some(row) => hydrate_metadata_row(row).map(Some),
            None => Ok(None),
        }
    }

    pub async fn sbom_coverage_stats(&self) -> Result<SbomCoverage> {
        let (total, with_sbom) = sqlx::query_as::<_, (i64, i64)>(
            r#"
            SELECT
                COUNT(*) as total,
                COALESCE(
                    SUM(CASE WHEN sbom_json IS NOT NULL AND sbom_json <> ''
                        THEN 1 ELSE 0 END),
                    0
                ) as with_sbom
            FROM gem_metadata
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .context("querying SBOM coverage (postgres)")?;

        Ok(SbomCoverage {
            metadata_rows: total.max(0) as u64,
            with_sbom: with_sbom.max(0) as u64,
        })
    }
}

impl CacheBackend for SqliteCacheBackend {
    async fn get(&self, key: &AssetKey<'_>) -> Result<Option<CachedAsset>> {
        let record = sqlx::query_as::<_, CachedAssetRow>(
            r#"
            SELECT path, sha256, size_bytes, last_accessed
            FROM cached_assets
            WHERE kind = ?1 AND name = ?2 AND version = ?3 AND
                  ((platform IS NULL AND ?4 IS NULL) OR platform = ?4)
            "#,
        )
        .bind(key.kind.as_str())
        .bind(key.name)
        .bind(key.version)
        .bind(key.platform)
        .fetch_optional(&self.pool)
        .await
        .context("fetching cached asset")?;

        if let Some(row) = record {
            self.touch(key).await?;
            Ok(Some(row.into()))
        } else {
            Ok(None)
        }
    }

    async fn insert_or_replace(
        &self,
        key: &AssetKey<'_>,
        path: &str,
        sha256: &str,
        size_bytes: u64,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO cached_assets(
                kind, name, version, platform, path, sha256, size_bytes, last_accessed
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, strftime('%Y-%m-%dT%H:%M:%fZ','now'))
            ON CONFLICT(kind, name, version, platform)
            DO UPDATE SET
                path = excluded.path,
                sha256 = excluded.sha256,
                size_bytes = excluded.size_bytes,
                last_accessed = excluded.last_accessed
            "#,
        )
        .bind(key.kind.as_str())
        .bind(key.name)
        .bind(key.version)
        .bind(key.platform)
        .bind(path)
        .bind(sha256)
        .bind(size_bytes as i64)
        .execute(&self.pool)
        .await
        .context("inserting cached asset")?;
        Ok(())
    }

    async fn get_all_gems(&self) -> Result<Vec<(String, String)>> {
        let rows = sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT DISTINCT name, version
            FROM cached_assets
            WHERE kind = 'gem'
            ORDER BY name, version
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .context("fetching all gems")?;

        Ok(rows)
    }

    async fn stats(&self) -> Result<IndexStats> {
        let total_assets: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cached_assets")
            .fetch_one(&self.pool)
            .await
            .context("counting cached assets")?;

        let gem_assets: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM cached_assets WHERE kind = 'gem'")
                .fetch_one(&self.pool)
                .await
                .context("counting gem assets")?;

        let spec_assets: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM cached_assets WHERE kind = 'gemspec'")
                .fetch_one(&self.pool)
                .await
                .context("counting gemspec assets")?;

        let unique_gems: i64 =
            sqlx::query_scalar("SELECT COUNT(DISTINCT name) FROM cached_assets WHERE kind = 'gem'")
                .fetch_one(&self.pool)
                .await
                .context("counting unique gems")?;

        let total_size_bytes: i64 =
            sqlx::query_scalar("SELECT COALESCE(SUM(size_bytes), 0) FROM cached_assets")
                .fetch_one(&self.pool)
                .await
                .context("summing cached asset sizes")?;

        let last_accessed: Option<String> =
            sqlx::query_scalar("SELECT MAX(last_accessed) FROM cached_assets")
                .fetch_one(&self.pool)
                .await
                .context("fetching last access timestamp")?;

        Ok(IndexStats {
            total_assets: total_assets.max(0) as u64,
            gem_assets: gem_assets.max(0) as u64,
            spec_assets: spec_assets.max(0) as u64,
            unique_gems: unique_gems.max(0) as u64,
            total_size_bytes: total_size_bytes.max(0) as u64,
            last_accessed,
        })
    }

    async fn catalog_upsert_names(&self, names: &[String]) -> Result<()> {
        if names.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for name in names {
            sqlx::query(
                r#"
                INSERT INTO catalog_gems(name, synced_at)
                VALUES(?1, strftime('%Y-%m-%dT%H:%M:%fZ','now'))
                ON CONFLICT(name) DO UPDATE SET synced_at = excluded.synced_at
                "#,
            )
            .bind(name)
            .execute(&mut *tx)
            .await
            .with_context(|| format!("upserting catalog entry {}", name))?;
        }
        tx.commit().await.context("committing catalog upsert")?;
        Ok(())
    }

    async fn catalog_total(&self) -> Result<u64> {
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM catalog_gems")
            .fetch_one(&self.pool)
            .await
            .context("counting catalog gems")?;
        Ok(total.max(0) as u64)
    }

    async fn catalog_page(&self, offset: i64, limit: i64) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            r#"
            SELECT name
            FROM catalog_gems
            ORDER BY name
            LIMIT ?1 OFFSET ?2
            "#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .context("fetching catalog page")?;
        Ok(rows)
    }

    async fn catalog_meta_get(&self, key: &str) -> Result<Option<String>> {
        let value =
            sqlx::query_scalar::<_, String>("SELECT value FROM catalog_meta WHERE key = ?1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await
                .context("fetching catalog meta value")?;
        Ok(value)
    }

    async fn catalog_meta_set(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO catalog_meta(key, value)
            VALUES(?1, ?2)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            "#,
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await
        .context("upserting catalog meta value")?;
        Ok(())
    }

    async fn upsert_metadata(&self, metadata: &GemMetadata) -> Result<()> {
        self.upsert_gem_metadata_record(metadata).await
    }

    async fn gem_metadata(
        &self,
        name: &str,
        version: &str,
        platform: Option<&str>,
    ) -> Result<Option<GemMetadata>> {
        self.fetch_gem_metadata(name, version, platform).await
    }

    async fn sbom_coverage(&self) -> Result<SbomCoverage> {
        let (total, with_sbom) = sqlx::query_as::<_, (i64, i64)>(
            r#"
            SELECT
                COUNT(*) as total,
                COALESCE(
                    SUM(CASE WHEN sbom_json IS NOT NULL AND sbom_json <> ''
                        THEN 1 ELSE 0 END),
                    0
                ) as with_sbom
            FROM gem_metadata
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .context("querying SBOM coverage (sqlite)")?;

        Ok(SbomCoverage {
            metadata_rows: total.max(0) as u64,
            with_sbom: with_sbom.max(0) as u64,
        })
    }

    async fn catalog_languages(&self) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, Option<String>>(
            r#"
            SELECT native_languages_json
            FROM gem_metadata
            WHERE native_languages_json IS NOT NULL AND native_languages_json <> ''
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .context("fetching native languages (sqlite)")?;

        parse_language_rows(rows)
    }

    async fn catalog_page_by_language(
        &self,
        language: &str,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<String>> {
        let pattern = format!("%\"{}\"%", language);
        let rows = sqlx::query_scalar::<_, String>(
            r#"
            SELECT DISTINCT name
            FROM gem_metadata
            WHERE native_languages_json LIKE ?1
            ORDER BY name
            LIMIT ?2 OFFSET ?3
            "#,
        )
        .bind(pattern)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .context("fetching catalog page by language (sqlite)")?;
        Ok(rows)
    }

    async fn catalog_total_by_language(&self, language: &str) -> Result<u64> {
        let pattern = format!("%\"{}\"%", language);
        let total: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(DISTINCT name)
            FROM gem_metadata
            WHERE native_languages_json LIKE ?1
            "#,
        )
        .bind(pattern)
        .fetch_one(&self.pool)
        .await
        .context("counting catalog gems by language (sqlite)")?;
        Ok(total.max(0) as u64)
    }

    // ==================== Quarantine Methods ====================

    async fn get_gem_version(
        &self,
        name: &str,
        version: &str,
        platform: Option<&str>,
    ) -> Result<Option<GemVersion>> {
        let row = sqlx::query_as::<_, GemVersionRow>(
            r#"
            SELECT id, name, version, platform, sha256, published_at, available_after,
                   status, status_reason, upstream_yanked, created_at, updated_at
            FROM gem_versions
            WHERE name = ?1
              AND version = ?2
              AND ((platform IS NULL AND ?3 IS NULL) OR platform = ?3)
            "#,
        )
        .bind(name)
        .bind(version)
        .bind(platform)
        .fetch_optional(&self.pool)
        .await
        .context("fetching gem version (sqlite)")?;

        Ok(row.map(Into::into))
    }

    async fn upsert_gem_version(&self, gem_version: &GemVersion) -> Result<()> {
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            INSERT INTO gem_versions (
                name, version, platform, sha256, published_at, available_after,
                status, status_reason, upstream_yanked, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            ON CONFLICT (name, version, platform)
            DO UPDATE SET
                sha256 = excluded.sha256,
                published_at = excluded.published_at,
                available_after = excluded.available_after,
                status = excluded.status,
                status_reason = excluded.status_reason,
                upstream_yanked = excluded.upstream_yanked,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&gem_version.name)
        .bind(&gem_version.version)
        .bind(gem_version.platform.as_deref())
        .bind(gem_version.sha256.as_deref())
        .bind(gem_version.published_at.to_rfc3339())
        .bind(gem_version.available_after.to_rfc3339())
        .bind(gem_version.status.to_string())
        .bind(gem_version.status_reason.as_deref())
        .bind(gem_version.upstream_yanked)
        .bind(gem_version.created_at.to_rfc3339())
        .bind(&now)
        .execute(&self.pool)
        .await
        .context("upserting gem version (sqlite)")?;

        Ok(())
    }

    async fn get_latest_available_version(
        &self,
        name: &str,
        platform: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<Option<GemVersion>> {
        let now_str = now.to_rfc3339();

        // Get all available versions and sort in Rust for proper semver comparison
        let rows = sqlx::query_as::<_, GemVersionRow>(
            r#"
            SELECT id, name, version, platform, sha256, published_at, available_after,
                   status, status_reason, upstream_yanked, created_at, updated_at
            FROM gem_versions
            WHERE name = ?1
              AND ((platform IS NULL AND ?2 IS NULL) OR platform = ?2)
              AND upstream_yanked = FALSE
              AND (status = 'available' OR status = 'pinned'
                   OR (status = 'quarantine' AND available_after <= ?3))
            "#,
        )
        .bind(name)
        .bind(platform)
        .bind(&now_str)
        .fetch_all(&self.pool)
        .await
        .context("fetching available versions (sqlite)")?;

        // Find the latest version using semver comparison
        let mut versions: Vec<GemVersion> = rows.into_iter().map(Into::into).collect();
        versions.sort_by(|a, b| {
            compare_versions(&b.version, &a.version) // Reverse for descending
        });

        Ok(versions.into_iter().next())
    }

    async fn get_quarantined_versions(
        &self,
        name: &str,
        now: DateTime<Utc>,
    ) -> Result<Vec<GemVersion>> {
        let now_str = now.to_rfc3339();

        let rows = sqlx::query_as::<_, GemVersionRow>(
            r#"
            SELECT id, name, version, platform, sha256, published_at, available_after,
                   status, status_reason, upstream_yanked, created_at, updated_at
            FROM gem_versions
            WHERE name = ?1
              AND status = 'quarantine'
              AND available_after > ?2
            ORDER BY version DESC
            "#,
        )
        .bind(name)
        .bind(&now_str)
        .fetch_all(&self.pool)
        .await
        .context("fetching quarantined versions (sqlite)")?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn update_version_status(
        &self,
        name: &str,
        version: &str,
        platform: Option<&str>,
        status: VersionStatus,
        reason: Option<String>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            UPDATE gem_versions
            SET status = ?1, status_reason = ?2, updated_at = ?3
            WHERE name = ?4
              AND version = ?5
              AND ((platform IS NULL AND ?6 IS NULL) OR platform = ?6)
            "#,
        )
        .bind(status.to_string())
        .bind(reason)
        .bind(&now)
        .bind(name)
        .bind(version)
        .bind(platform)
        .execute(&self.pool)
        .await
        .context("updating version status (sqlite)")?;

        Ok(())
    }

    async fn promote_expired_quarantines(&self, now: DateTime<Utc>) -> Result<u64> {
        let now_str = now.to_rfc3339();

        let result = sqlx::query(
            r#"
            UPDATE gem_versions
            SET status = 'available', status_reason = 'auto-promoted', updated_at = ?1
            WHERE status = 'quarantine'
              AND available_after <= ?2
            "#,
        )
        .bind(&now_str)
        .bind(&now_str)
        .execute(&self.pool)
        .await
        .context("promoting expired quarantines (sqlite)")?;

        Ok(result.rows_affected())
    }

    async fn mark_yanked(&self, name: &str, version: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            UPDATE gem_versions
            SET status = 'yanked', upstream_yanked = TRUE, updated_at = ?1
            WHERE name = ?2 AND version = ?3
            "#,
        )
        .bind(&now)
        .bind(name)
        .bind(version)
        .execute(&self.pool)
        .await
        .context("marking version yanked (sqlite)")?;

        Ok(())
    }

    async fn get_all_quarantined(&self, limit: u32, offset: u32) -> Result<Vec<GemVersion>> {
        let rows = sqlx::query_as::<_, GemVersionRow>(
            r#"
            SELECT id, name, version, platform, sha256, published_at, available_after,
                   status, status_reason, upstream_yanked, created_at, updated_at
            FROM gem_versions
            WHERE status = 'quarantine'
            ORDER BY available_after ASC
            LIMIT ?1 OFFSET ?2
            "#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .context("fetching all quarantined (sqlite)")?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn quarantine_stats(&self) -> Result<QuarantineStats> {
        let now = Utc::now();
        let today_end = (now + Duration::days(1)).to_rfc3339();
        let week_end = (now + Duration::days(7)).to_rfc3339();
        let now_str = now.to_rfc3339();

        let (quarantined, available, yanked, pinned): (i64, i64, i64, i64) = sqlx::query_as(
            r#"
            SELECT
                COALESCE(SUM(CASE WHEN status = 'quarantine' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN status = 'available' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN status = 'yanked' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN status = 'pinned' THEN 1 ELSE 0 END), 0)
            FROM gem_versions
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .context("fetching quarantine counts (sqlite)")?;

        let releasing_today: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM gem_versions
            WHERE status = 'quarantine'
              AND available_after > ?1
              AND available_after <= ?2
            "#,
        )
        .bind(&now_str)
        .bind(&today_end)
        .fetch_one(&self.pool)
        .await
        .context("counting versions releasing today (sqlite)")?;

        let releasing_week: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM gem_versions
            WHERE status = 'quarantine'
              AND available_after > ?1
              AND available_after <= ?2
            "#,
        )
        .bind(&now_str)
        .bind(&week_end)
        .fetch_one(&self.pool)
        .await
        .context("counting versions releasing this week (sqlite)")?;

        Ok(QuarantineStats {
            total_quarantined: quarantined.max(0) as u64,
            total_available: available.max(0) as u64,
            total_yanked: yanked.max(0) as u64,
            total_pinned: pinned.max(0) as u64,
            versions_releasing_today: releasing_today.max(0) as u64,
            versions_releasing_this_week: releasing_week.max(0) as u64,
        })
    }

    async fn get_gem_versions_for_index(&self, name: &str) -> Result<Vec<GemVersion>> {
        let rows = sqlx::query_as::<_, GemVersionRow>(
            r#"
            SELECT id, name, version, platform, sha256, published_at, available_after,
                   status, status_reason, upstream_yanked, created_at, updated_at
            FROM gem_versions
            WHERE name = ?1
            ORDER BY version DESC
            "#,
        )
        .bind(name)
        .fetch_all(&self.pool)
        .await
        .context("fetching gem versions for index (sqlite)")?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn quarantine_table_exists(&self) -> Result<bool> {
        let exists: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM sqlite_master
            WHERE type = 'table' AND name = 'gem_versions'
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .context("checking quarantine table exists (sqlite)")?;

        Ok(exists > 0)
    }

    async fn run_quarantine_migrations(&self) -> Result<()> {
        // Create gem_versions table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS gem_versions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                version TEXT NOT NULL,
                platform TEXT,
                sha256 TEXT,
                published_at TEXT NOT NULL,
                available_after TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'quarantine',
                status_reason TEXT,
                upstream_yanked INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(name, version, platform)
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .context("creating gem_versions table (sqlite)")?;

        // Create indexes
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_gem_versions_name ON gem_versions(name)",
        )
        .execute(&self.pool)
        .await
        .context("creating name index (sqlite)")?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_gem_versions_status ON gem_versions(status)",
        )
        .execute(&self.pool)
        .await
        .context("creating status index (sqlite)")?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_gv_available ON gem_versions(available_after)",
        )
        .execute(&self.pool)
        .await
        .context("creating available_after index (sqlite)")?;

        Ok(())
    }
}

/// Compare two version strings using semver when possible.
fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    // Try semver parsing first
    match (semver::Version::parse(a), semver::Version::parse(b)) {
        (Ok(va), Ok(vb)) => va.cmp(&vb),
        // Fall back to string comparison if semver fails
        _ => a.cmp(b),
    }
}
