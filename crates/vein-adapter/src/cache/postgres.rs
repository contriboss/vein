use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{
    PgPool, Transaction,
    postgres::{PgPoolOptions, Postgres},
};

use super::{
    CacheBackend,
    models::{DbGemMetadataRow, PostgresCachedAssetRow, format_timestamp},
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
}
