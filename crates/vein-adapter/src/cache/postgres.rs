use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use sqlx::{
    PgPool, Transaction,
    postgres::{PgPoolOptions, Postgres},
};

use super::{
    CacheBackend, GemVersion, QuarantineStats, VersionStatus,
    models::{DbGemMetadataRow, PostgresCachedAssetRow, PostgresGemVersionRow, format_timestamp},
    serialization::{hydrate_metadata_row, parse_language_rows, prepare_metadata_strings},
    types::{AssetKey, CachedAsset, GemMetadata, IndexStats, SbomCoverage},
};

#[derive(Debug, Clone)]
pub struct PostgresCacheBackend {
    pool: PgPool,
}

impl PostgresCacheBackend {
    pub async fn connect(url: &str, max_connections: u32) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections.max(1))
            .connect(url)
            .await
            .with_context(|| format!("connecting to postgres database {}", url))?;
        Ok(Self { pool })
    }

    async fn touch(&self, key: &AssetKey<'_>) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE cached_assets
            SET last_accessed = NOW()
            WHERE kind = $1 AND name = $2 AND version = $3 AND
                  ((platform IS NULL AND $4 IS NULL) OR platform = $4)
            "#,
        )
        .bind(key.kind.as_str())
        .bind(key.name)
        .bind(key.version)
        .bind(key.platform)
        .execute(&self.pool)
        .await
        .context("updating last_accessed (postgres)")?;
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
            WHERE name = $1
              AND version = $2
              AND ((platform IS NULL AND $3 IS NULL) OR platform = $3)
            "#,
        )
        .bind(name)
        .bind(version)
        .bind(platform)
        .fetch_optional(&self.pool)
        .await
        .context("fetching gem metadata record (postgres)")?;

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

    pub async fn catalog_languages_list(&self) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, Option<String>>(
            r#"
            SELECT native_languages_json
            FROM gem_metadata
            WHERE native_languages_json IS NOT NULL AND native_languages_json <> ''
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .context("fetching native languages (postgres)")?;

        parse_language_rows(rows)
    }

    async fn begin_tx(&self) -> Result<Transaction<'_, Postgres>> {
        self.pool
            .begin()
            .await
            .context("starting postgres transaction")
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
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                $11, $12, $13, $14, $15, $16, $17, $18, $19, $20,
                $21, $22, $23, $24, $25, $26, $27, $28, $29, $30
            )
            ON CONFLICT ON CONSTRAINT gem_metadata_pkey
            DO UPDATE SET
                summary = EXCLUDED.summary,
                description = EXCLUDED.description,
                licenses = EXCLUDED.licenses,
                authors = EXCLUDED.authors,
                emails = EXCLUDED.emails,
                homepage = EXCLUDED.homepage,
                documentation_url = EXCLUDED.documentation_url,
                changelog_url = EXCLUDED.changelog_url,
                source_code_url = EXCLUDED.source_code_url,
                bug_tracker_url = EXCLUDED.bug_tracker_url,
                wiki_url = EXCLUDED.wiki_url,
                funding_url = EXCLUDED.funding_url,
                metadata_json = EXCLUDED.metadata_json,
                dependencies_json = EXCLUDED.dependencies_json,
                executables_json = EXCLUDED.executables_json,
                extensions_json = EXCLUDED.extensions_json,
                native_languages_json = EXCLUDED.native_languages_json,
                has_native_extensions = EXCLUDED.has_native_extensions,
                has_embedded_binaries = EXCLUDED.has_embedded_binaries,
                required_ruby_version = EXCLUDED.required_ruby_version,
                required_rubygems_version = EXCLUDED.required_rubygems_version,
                rubygems_version = EXCLUDED.rubygems_version,
                specification_version = EXCLUDED.specification_version,
                built_at = EXCLUDED.built_at,
                size_bytes = EXCLUDED.size_bytes,
                sha256 = EXCLUDED.sha256,
                sbom_json = EXCLUDED.sbom_json
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
        .context("upserting gem metadata (postgres)")?;

        Ok(())
    }
}

#[async_trait]
impl CacheBackend for PostgresCacheBackend {
    async fn get(&self, key: &AssetKey<'_>) -> Result<Option<CachedAsset>> {
        let record = sqlx::query_as::<_, PostgresCachedAssetRow>(
            r#"
            SELECT path, sha256, size_bytes, last_accessed
            FROM cached_assets
            WHERE kind = $1 AND name = $2 AND version = $3 AND
                  ((platform IS NULL AND $4 IS NULL) OR platform = $4)
            "#,
        )
        .bind(key.kind.as_str())
        .bind(key.name)
        .bind(key.version)
        .bind(key.platform)
        .fetch_optional(&self.pool)
        .await
        .context("fetching cached asset (postgres)")?;

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
            VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
            ON CONFLICT ON CONSTRAINT cached_assets_unique
            DO UPDATE SET
                path = EXCLUDED.path,
                sha256 = EXCLUDED.sha256,
                size_bytes = EXCLUDED.size_bytes,
                last_accessed = EXCLUDED.last_accessed
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
        .context("inserting cached asset (postgres)")?;
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
        .context("fetching all gems (postgres)")?;

        Ok(rows)
    }

    async fn stats(&self) -> Result<IndexStats> {
        let total_assets: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cached_assets")
            .fetch_one(&self.pool)
            .await
            .context("counting cached assets (postgres)")?;

        let gem_assets: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM cached_assets WHERE kind = 'gem'")
                .fetch_one(&self.pool)
                .await
                .context("counting gem assets (postgres)")?;

        let spec_assets: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM cached_assets WHERE kind = 'gemspec'")
                .fetch_one(&self.pool)
                .await
                .context("counting gemspec assets (postgres)")?;

        let unique_gems: i64 =
            sqlx::query_scalar("SELECT COUNT(DISTINCT name) FROM cached_assets WHERE kind = 'gem'")
                .fetch_one(&self.pool)
                .await
                .context("counting unique gems (postgres)")?;

        let total_size_bytes: i64 =
            sqlx::query_scalar("SELECT COALESCE(SUM(size_bytes), 0) FROM cached_assets")
                .fetch_one(&self.pool)
                .await
                .context("summing cached asset sizes (postgres)")?;

        let last_accessed: Option<DateTime<Utc>> =
            sqlx::query_scalar("SELECT MAX(last_accessed) FROM cached_assets")
                .fetch_one(&self.pool)
                .await
                .context("fetching last access timestamp (postgres)")?;

        Ok(IndexStats {
            total_assets: total_assets.max(0) as u64,
            gem_assets: gem_assets.max(0) as u64,
            spec_assets: spec_assets.max(0) as u64,
            unique_gems: unique_gems.max(0) as u64,
            total_size_bytes: total_size_bytes.max(0) as u64,
            last_accessed: last_accessed.map(format_timestamp),
        })
    }

    async fn catalog_upsert_names(&self, names: &[String]) -> Result<()> {
        if names.is_empty() {
            return Ok(());
        }
        let mut tx = self.begin_tx().await?;
        for name in names {
            sqlx::query(
                r#"
                INSERT INTO catalog_gems(name, synced_at)
                VALUES($1, NOW())
                ON CONFLICT(name) DO UPDATE SET synced_at = EXCLUDED.synced_at
                "#,
            )
            .bind(name)
            .execute(&mut *tx)
            .await
            .with_context(|| format!("upserting catalog entry {} (postgres)", name))?;
        }
        tx.commit()
            .await
            .context("committing catalog upsert (postgres)")?;
        Ok(())
    }

    async fn catalog_total(&self) -> Result<u64> {
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM catalog_gems")
            .fetch_one(&self.pool)
            .await
            .context("counting catalog gems (postgres)")?;
        Ok(total.max(0) as u64)
    }

    async fn catalog_page(&self, offset: i64, limit: i64) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            r#"
            SELECT name
            FROM catalog_gems
            ORDER BY name
            LIMIT $1 OFFSET $2
            "#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .context("fetching catalog page (postgres)")?;
        Ok(rows)
    }

    async fn catalog_meta_get(&self, key: &str) -> Result<Option<String>> {
        let value =
            sqlx::query_scalar::<_, String>("SELECT value FROM catalog_meta WHERE key = $1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await
                .context("fetching catalog meta value (postgres)")?;
        Ok(value)
    }

    async fn catalog_meta_set(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO catalog_meta(key, value)
            VALUES($1, $2)
            ON CONFLICT(key) DO UPDATE SET value = EXCLUDED.value
            "#,
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await
        .context("upserting catalog meta value (postgres)")?;
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
        self.sbom_coverage_stats().await
    }

    async fn catalog_languages(&self) -> Result<Vec<String>> {
        self.catalog_languages_list().await
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
            WHERE native_languages_json LIKE $1
            ORDER BY name
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(pattern)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .context("fetching catalog page by language (postgres)")?;
        Ok(rows)
    }

    async fn catalog_total_by_language(&self, language: &str) -> Result<u64> {
        let pattern = format!("%\"{}\"%", language);
        let total: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(DISTINCT name)
            FROM gem_metadata
            WHERE native_languages_json LIKE $1
            "#,
        )
        .bind(pattern)
        .fetch_one(&self.pool)
        .await
        .context("counting catalog gems by language (postgres)")?;
        Ok(total.max(0) as u64)
    }

    // ==================== Quarantine Methods ====================

    async fn get_gem_version(
        &self,
        name: &str,
        version: &str,
        platform: Option<&str>,
    ) -> Result<Option<GemVersion>> {
        let row = sqlx::query_as::<_, PostgresGemVersionRow>(
            r#"
            SELECT id, name, version, platform, sha256, published_at, available_after,
                   status, status_reason, upstream_yanked, created_at, updated_at
            FROM gem_versions
            WHERE name = $1
              AND version = $2
              AND ((platform IS NULL AND $3 IS NULL) OR platform = $3)
            "#,
        )
        .bind(name)
        .bind(version)
        .bind(platform)
        .fetch_optional(&self.pool)
        .await
        .context("fetching gem version (postgres)")?;

        Ok(row.map(Into::into))
    }

    async fn upsert_gem_version(&self, gem_version: &GemVersion) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO gem_versions (
                name, version, platform, sha256, published_at, available_after,
                status, status_reason, upstream_yanked, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NOW())
            ON CONFLICT ON CONSTRAINT gem_versions_unique
            DO UPDATE SET
                sha256 = EXCLUDED.sha256,
                published_at = EXCLUDED.published_at,
                available_after = EXCLUDED.available_after,
                status = EXCLUDED.status,
                status_reason = EXCLUDED.status_reason,
                upstream_yanked = EXCLUDED.upstream_yanked,
                updated_at = NOW()
            "#,
        )
        .bind(&gem_version.name)
        .bind(&gem_version.version)
        .bind(gem_version.platform.as_deref())
        .bind(gem_version.sha256.as_deref())
        .bind(gem_version.published_at)
        .bind(gem_version.available_after)
        .bind(gem_version.status.to_string())
        .bind(gem_version.status_reason.as_deref())
        .bind(gem_version.upstream_yanked)
        .bind(gem_version.created_at)
        .execute(&self.pool)
        .await
        .context("upserting gem version (postgres)")?;

        Ok(())
    }

    async fn get_latest_available_version(
        &self,
        name: &str,
        platform: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<Option<GemVersion>> {
        // Get all available versions and sort in Rust for proper semver comparison
        let rows = sqlx::query_as::<_, PostgresGemVersionRow>(
            r#"
            SELECT id, name, version, platform, sha256, published_at, available_after,
                   status, status_reason, upstream_yanked, created_at, updated_at
            FROM gem_versions
            WHERE name = $1
              AND ((platform IS NULL AND $2 IS NULL) OR platform = $2)
              AND upstream_yanked = FALSE
              AND (status = 'available' OR status = 'pinned'
                   OR (status = 'quarantine' AND available_after <= $3))
            "#,
        )
        .bind(name)
        .bind(platform)
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .context("fetching available versions (postgres)")?;

        // Find the latest version using semver comparison
        let mut versions: Vec<GemVersion> = rows.into_iter().map(Into::into).collect();
        versions.sort_by(|a, b| compare_versions(&b.version, &a.version));

        Ok(versions.into_iter().next())
    }

    async fn get_quarantined_versions(
        &self,
        name: &str,
        now: DateTime<Utc>,
    ) -> Result<Vec<GemVersion>> {
        let rows = sqlx::query_as::<_, PostgresGemVersionRow>(
            r#"
            SELECT id, name, version, platform, sha256, published_at, available_after,
                   status, status_reason, upstream_yanked, created_at, updated_at
            FROM gem_versions
            WHERE name = $1
              AND status = 'quarantine'
              AND available_after > $2
            ORDER BY version DESC
            "#,
        )
        .bind(name)
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .context("fetching quarantined versions (postgres)")?;

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
        sqlx::query(
            r#"
            UPDATE gem_versions
            SET status = $1, status_reason = $2, updated_at = NOW()
            WHERE name = $3
              AND version = $4
              AND ((platform IS NULL AND $5 IS NULL) OR platform = $5)
            "#,
        )
        .bind(status.to_string())
        .bind(reason)
        .bind(name)
        .bind(version)
        .bind(platform)
        .execute(&self.pool)
        .await
        .context("updating version status (postgres)")?;

        Ok(())
    }

    async fn promote_expired_quarantines(&self, now: DateTime<Utc>) -> Result<u64> {
        let result = sqlx::query(
            r#"
            UPDATE gem_versions
            SET status = 'available', status_reason = 'auto-promoted', updated_at = NOW()
            WHERE status = 'quarantine'
              AND available_after <= $1
            "#,
        )
        .bind(now)
        .execute(&self.pool)
        .await
        .context("promoting expired quarantines (postgres)")?;

        Ok(result.rows_affected())
    }

    async fn mark_yanked(&self, name: &str, version: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE gem_versions
            SET status = 'yanked', upstream_yanked = TRUE, updated_at = NOW()
            WHERE name = $1 AND version = $2
            "#,
        )
        .bind(name)
        .bind(version)
        .execute(&self.pool)
        .await
        .context("marking version yanked (postgres)")?;

        Ok(())
    }

    async fn get_all_quarantined(&self, limit: u32, offset: u32) -> Result<Vec<GemVersion>> {
        let rows = sqlx::query_as::<_, PostgresGemVersionRow>(
            r#"
            SELECT id, name, version, platform, sha256, published_at, available_after,
                   status, status_reason, upstream_yanked, created_at, updated_at
            FROM gem_versions
            WHERE status = 'quarantine'
            ORDER BY available_after ASC
            LIMIT $1 OFFSET $2
            "#,
        )
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await
        .context("fetching all quarantined (postgres)")?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn quarantine_stats(&self) -> Result<QuarantineStats> {
        let now = Utc::now();
        let today_end = now + Duration::days(1);
        let week_end = now + Duration::days(7);

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
        .context("fetching quarantine counts (postgres)")?;

        let releasing_today: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM gem_versions
            WHERE status = 'quarantine'
              AND available_after > $1
              AND available_after <= $2
            "#,
        )
        .bind(now)
        .bind(today_end)
        .fetch_one(&self.pool)
        .await
        .context("counting versions releasing today (postgres)")?;

        let releasing_week: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM gem_versions
            WHERE status = 'quarantine'
              AND available_after > $1
              AND available_after <= $2
            "#,
        )
        .bind(now)
        .bind(week_end)
        .fetch_one(&self.pool)
        .await
        .context("counting versions releasing this week (postgres)")?;

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
        let rows = sqlx::query_as::<_, PostgresGemVersionRow>(
            r#"
            SELECT id, name, version, platform, sha256, published_at, available_after,
                   status, status_reason, upstream_yanked, created_at, updated_at
            FROM gem_versions
            WHERE name = $1
            ORDER BY version DESC
            "#,
        )
        .bind(name)
        .fetch_all(&self.pool)
        .await
        .context("fetching gem versions for index (postgres)")?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn quarantine_table_exists(&self) -> Result<bool> {
        let exists: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS (
                SELECT FROM information_schema.tables
                WHERE table_name = 'gem_versions'
            )
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .context("checking quarantine table exists (postgres)")?;

        Ok(exists)
    }

    async fn run_quarantine_migrations(&self) -> Result<()> {
        // Create gem_versions table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS gem_versions (
                id BIGSERIAL PRIMARY KEY,
                name TEXT NOT NULL,
                version TEXT NOT NULL,
                platform TEXT,
                sha256 TEXT,
                published_at TIMESTAMPTZ NOT NULL,
                available_after TIMESTAMPTZ NOT NULL,
                status TEXT NOT NULL DEFAULT 'quarantine',
                status_reason TEXT,
                upstream_yanked BOOLEAN NOT NULL DEFAULT FALSE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                CONSTRAINT gem_versions_unique UNIQUE (name, version, platform)
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .context("creating gem_versions table (postgres)")?;

        // Create indexes
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_gem_versions_name ON gem_versions(name)",
        )
        .execute(&self.pool)
        .await
        .context("creating name index (postgres)")?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_gem_versions_status ON gem_versions(status)",
        )
        .execute(&self.pool)
        .await
        .context("creating status index (postgres)")?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_gv_available ON gem_versions(available_after)",
        )
        .execute(&self.pool)
        .await
        .context("creating available_after index (postgres)")?;

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
