use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
#[cfg(any(test, feature = "test-support"))]
use sqlx::{Connection as _, PgConnection, postgres::PgConnectOptions};
use sqlx::{
    PgPool, Transaction,
    postgres::{PgPoolOptions, Postgres},
};

use super::{
    CacheBackendTrait, GemVersion, QuarantineStats, VersionStatus,
    backend_common::{
        build_index_stats, build_quarantine_stats, build_sbom_coverage, into_gem_versions,
        json_array_like_pattern, latest_gem_version, newest_first, search_like_pattern,
        search_prefix_pattern,
    },
    models::{CachedAssetRow, CachedVersionRow, DbGemMetadataRow, GemVersionRow, format_timestamp},
    serialization::{hydrate_metadata_row, parse_language_rows, prepare_metadata_strings},
    types::{
        AssetKey, AssetKind, CachedAsset, CachedVersion, GemMetadata, IndexStats, SbomCoverage,
    },
};

/// Ranks name matches: exact, then case-insensitive exact, then prefix, then
/// the rest, in code-point order within each (`COLLATE ucs_basic`, so results
/// do not depend on the database collation). Binds `$2` limit, `$3` query,
/// `$4` prefix pattern.
macro_rules! rank_by_name {
    () => {
        "ORDER BY CASE
             WHEN name = $3 THEN 0
             WHEN lower(name) = lower($3) THEN 1
             WHEN name ILIKE $4 THEN 2
             ELSE 3
         END,
         name COLLATE ucs_basic
         LIMIT $2"
    };
}

#[derive(Debug, Clone)]
pub struct PostgresCacheBackend {
    pool: PgPool,
    /// Drops the per-test schema once the last clone is gone.
    #[cfg(any(test, feature = "test-support"))]
    isolation: Option<std::sync::Arc<test_schema::SchemaGuard>>,
}

impl PostgresCacheBackend {
    pub async fn connect(url: &str, max_connections: u32) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections.max(1))
            .acquire_timeout(std::time::Duration::from_secs(5))
            .idle_timeout(std::time::Duration::from_secs(60))
            .test_before_acquire(true)
            .connect(url)
            .await
            .with_context(|| format!("connecting to postgres database {url}"))?;

        let backend = Self::migrate(pool).await?;
        Ok(backend)
    }

    /// A backend on a fresh, uniquely named schema of the database at `url`,
    /// so concurrent tests never see each other's rows. The schema is dropped
    /// when the last clone of the backend goes away; schemas a killed run
    /// left behind are swept once per process.
    ///
    /// # Errors
    ///
    /// When `url` does not parse, the database is unreachable, the schema
    /// cannot be created, or migrations fail.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn connect_isolated(url: &str) -> Result<Self> {
        let options: PgConnectOptions = url
            .parse()
            .with_context(|| format!("parsing postgres url {url}"))?;
        let schema = test_schema::fresh_name();

        let mut admin = PgConnection::connect_with(&options)
            .await
            .with_context(|| format!("connecting to postgres database {url}"))?;
        test_schema::sweep_stale(&mut admin).await?;
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&mut admin)
            .await
            .with_context(|| format!("creating test schema {schema}"))?;
        admin.close().await.context("closing admin connection")?;

        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options.clone().options([("search_path", schema.as_str())]))
            .await
            .with_context(|| format!("connecting to test schema {schema}"))?;

        let mut backend = Self::migrate(pool).await?;
        backend.isolation = Some(std::sync::Arc::new(test_schema::SchemaGuard::new(
            options, schema,
        )));
        Ok(backend)
    }

    async fn migrate(pool: PgPool) -> Result<Self> {
        sqlx::migrate!("./migrations/postgres")
            .run(&pool)
            .await
            .context("running postgres migrations")?;

        Ok(Self {
            pool,
            #[cfg(any(test, feature = "test-support"))]
            isolation: None,
        })
    }

    async fn touch(&self, key: &AssetKey<'_>) -> Result<()> {
        sqlx::query(
            r"
            UPDATE cached_assets
            SET last_accessed = NOW()
            WHERE kind = $1 AND name = $2 AND version = $3 AND
                  ((platform IS NULL AND $4 IS NULL) OR platform = $4)
            ",
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
            r"
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
              AND platform = $3
            ",
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
            r"
            SELECT
                COUNT(*) as total,
                COALESCE(
                    SUM(CASE WHEN sbom_json IS NOT NULL AND sbom_json <> ''
                        THEN 1 ELSE 0 END),
                    0
                ) as with_sbom
            FROM gem_metadata
            ",
        )
        .fetch_one(&self.pool)
        .await
        .context("querying SBOM coverage (postgres)")?;

        Ok(build_sbom_coverage(total, with_sbom))
    }

    pub async fn catalog_languages_list(&self) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, Option<String>>(
            r"
            SELECT native_languages_json
            FROM gem_metadata
            WHERE native_languages_json IS NOT NULL AND native_languages_json <> ''
            ",
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
            r"
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
            ",
        )
        .bind(&metadata.name)
        .bind(&metadata.version)
        .bind(&metadata.platform)
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

impl CacheBackendTrait for PostgresCacheBackend {
    async fn get(&self, key: &AssetKey<'_>) -> Result<Option<CachedAsset>> {
        let record = sqlx::query_as::<_, CachedAssetRow>(
            r"
            SELECT path, sha256, size_bytes, last_accessed
            FROM cached_assets
            WHERE kind = $1 AND name = $2 AND version = $3 AND
                  ((platform IS NULL AND $4 IS NULL) OR platform = $4)
            ",
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
            r"
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
            ",
        )
        .bind(key.kind.as_str())
        .bind(key.name)
        .bind(key.version)
        .bind(key.platform)
        .bind(path)
        .bind(sha256)
        .bind(i64::try_from(size_bytes).context("asset size exceeds i64")?)
        .execute(&self.pool)
        .await
        .context("inserting cached asset (postgres)")?;
        Ok(())
    }

    async fn get_all_gems(&self) -> Result<Vec<(String, String)>> {
        let rows = sqlx::query_as::<_, (String, String)>(
            r"
            SELECT name, version
            FROM cached_assets
            WHERE kind = 'gem'
            GROUP BY name, version
            ORDER BY name COLLATE ucs_basic, version COLLATE ucs_basic
            ",
        )
        .fetch_all(&self.pool)
        .await
        .context("fetching all gems (postgres)")?;

        Ok(rows)
    }

    async fn cached_versions(&self, kind: AssetKind, name: &str) -> Result<Vec<CachedVersion>> {
        let rows = sqlx::query_as::<_, CachedVersionRow>(
            r"
            SELECT version, platform, sha256, size_bytes, last_accessed
            FROM cached_assets
            WHERE kind = $1 AND name = $2
            ",
        )
        .bind(kind.as_str())
        .bind(name)
        .fetch_all(&self.pool)
        .await
        .context("fetching cached versions (postgres)")?;
        Ok(newest_first(rows, |cached: &CachedVersion| &cached.version))
    }

    async fn search_cached(&self, kind: AssetKind, query: &str, limit: i64) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(concat!(
            "SELECT name FROM cached_assets WHERE name ILIKE $1 AND kind = $5 GROUP BY name ",
            rank_by_name!()
        ))
        .bind(search_like_pattern(query))
        .bind(limit)
        .bind(query)
        .bind(search_prefix_pattern(query))
        .bind(kind.as_str())
        .fetch_all(&self.pool)
        .await
        .context("searching cached packages (postgres)")?;
        Ok(rows)
    }

    async fn stats(&self) -> Result<IndexStats> {
        let total_assets: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cached_assets")
            .fetch_one(&self.pool)
            .await
            .context("counting cached assets (postgres)")?;

        let rubygems_assets: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM cached_assets WHERE kind IN ('gem', 'gemspec')",
        )
        .fetch_one(&self.pool)
        .await
        .context("counting rubygems assets (postgres)")?;

        let crate_assets: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM cached_assets WHERE kind = 'crate'")
                .fetch_one(&self.pool)
                .await
                .context("counting crate assets (postgres)")?;

        let npm_assets: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM cached_assets WHERE kind = 'npm'")
                .fetch_one(&self.pool)
                .await
                .context("counting npm assets (postgres)")?;

        let unique_packages: i64 = sqlx::query_scalar(
            r"
            SELECT COUNT(
                DISTINCT CASE
                    WHEN kind IN ('gem', 'gemspec') THEN 'rubygems:' || name
                    WHEN kind = 'crate' THEN 'crates:' || name
                    WHEN kind = 'npm' THEN 'npm:' || name
                    ELSE NULL
                END
            )
            FROM cached_assets
            ",
        )
        .fetch_one(&self.pool)
        .await
        .context("counting unique cached packages (postgres)")?;

        let total_size_bytes: i64 =
            sqlx::query_scalar("SELECT COALESCE(SUM(size_bytes), 0)::BIGINT FROM cached_assets")
                .fetch_one(&self.pool)
                .await
                .context("summing cached asset sizes (postgres)")?;

        let last_accessed: Option<DateTime<Utc>> =
            sqlx::query_scalar("SELECT MAX(last_accessed) FROM cached_assets")
                .fetch_one(&self.pool)
                .await
                .context("fetching last access timestamp (postgres)")?;

        Ok(build_index_stats(
            total_assets,
            rubygems_assets,
            crate_assets,
            npm_assets,
            unique_packages,
            total_size_bytes,
            last_accessed.map(format_timestamp),
        ))
    }

    async fn catalog_upsert_names(&self, names: &[String]) -> Result<()> {
        if names.is_empty() {
            return Ok(());
        }
        let mut tx = self.begin_tx().await?;
        for name in names {
            sqlx::query(
                r"
                INSERT INTO catalog_gems(name, synced_at)
                VALUES($1, NOW())
                ON CONFLICT(name) DO UPDATE SET synced_at = EXCLUDED.synced_at
                ",
            )
            .bind(name)
            .execute(&mut *tx)
            .await
            .with_context(|| format!("upserting catalog entry {name} (postgres)"))?;
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
        Ok(total.max(0).cast_unsigned())
    }

    async fn catalog_page(&self, offset: i64, limit: i64) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            r"
            SELECT name
            FROM catalog_gems
            ORDER BY name COLLATE ucs_basic
            LIMIT $1 OFFSET $2
            ",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .context("fetching catalog page (postgres)")?;
        Ok(rows)
    }

    async fn catalog_search(&self, query: &str, limit: i64) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(concat!(
            "SELECT name FROM catalog_gems WHERE name ILIKE $1 ",
            rank_by_name!()
        ))
        .bind(search_like_pattern(query))
        .bind(limit)
        .bind(query)
        .bind(search_prefix_pattern(query))
        .fetch_all(&self.pool)
        .await
        .context("searching catalog (postgres)")?;
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
            r"
            INSERT INTO catalog_meta(key, value)
            VALUES($1, $2)
            ON CONFLICT(key) DO UPDATE SET value = EXCLUDED.value
            ",
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
        let pattern = json_array_like_pattern(language);
        let rows = sqlx::query_scalar::<_, String>(
            r"
            SELECT name
            FROM gem_metadata
            WHERE native_languages_json LIKE $1
            GROUP BY name
            ORDER BY name COLLATE ucs_basic
            LIMIT $2 OFFSET $3
            ",
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
        let pattern = json_array_like_pattern(language);
        let total: i64 = sqlx::query_scalar(
            r"
            SELECT COUNT(DISTINCT name)
            FROM gem_metadata
            WHERE native_languages_json LIKE $1
            ",
        )
        .bind(pattern)
        .fetch_one(&self.pool)
        .await
        .context("counting catalog gems by language (postgres)")?;
        Ok(total.max(0).cast_unsigned())
    }

    // ==================== Quarantine Methods ====================

    async fn get_gem_version(
        &self,
        name: &str,
        version: &str,
        platform: Option<&str>,
    ) -> Result<Option<GemVersion>> {
        let row = sqlx::query_as::<_, GemVersionRow>(
            r"
            SELECT id, name, version, platform, sha256, published_at, available_after,
                   status, status_reason, upstream_yanked, created_at, updated_at
            FROM gem_versions
            WHERE name = $1
              AND version = $2
              AND ((platform IS NULL AND $3 IS NULL) OR platform = $3)
            ",
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
            r"
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
            ",
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
        // Get all available versions and sort in Rust by RubyGems version order
        let rows = sqlx::query_as::<_, GemVersionRow>(
            r"
            SELECT id, name, version, platform, sha256, published_at, available_after,
                   status, status_reason, upstream_yanked, created_at, updated_at
            FROM gem_versions
            WHERE name = $1
              AND ((platform IS NULL AND $2 IS NULL) OR platform = $2)
              AND upstream_yanked = FALSE
              AND (status = 'available' OR status = 'pinned'
                   OR (status = 'quarantine' AND available_after <= $3))
            ",
        )
        .bind(name)
        .bind(platform)
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .context("fetching available versions (postgres)")?;

        Ok(latest_gem_version(rows))
    }

    async fn get_quarantined_versions(
        &self,
        name: &str,
        now: DateTime<Utc>,
    ) -> Result<Vec<GemVersion>> {
        let rows = sqlx::query_as::<_, GemVersionRow>(
            r"
            SELECT id, name, version, platform, sha256, published_at, available_after,
                   status, status_reason, upstream_yanked, created_at, updated_at
            FROM gem_versions
            WHERE name = $1
              AND status = 'quarantine'
              AND available_after > $2
            ORDER BY version COLLATE ucs_basic DESC
            ",
        )
        .bind(name)
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .context("fetching quarantined versions (postgres)")?;

        Ok(into_gem_versions(rows))
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
            r"
            UPDATE gem_versions
            SET status = $1, status_reason = $2, updated_at = NOW()
            WHERE name = $3
              AND version = $4
              AND ((platform IS NULL AND $5 IS NULL) OR platform = $5)
            ",
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
            r"
            UPDATE gem_versions
            SET status = 'available', status_reason = 'auto-promoted', updated_at = NOW()
            WHERE status = 'quarantine'
              AND available_after <= $1
            ",
        )
        .bind(now)
        .execute(&self.pool)
        .await
        .context("promoting expired quarantines (postgres)")?;

        Ok(result.rows_affected())
    }

    async fn mark_yanked(&self, name: &str, version: &str) -> Result<()> {
        sqlx::query(
            r"
            UPDATE gem_versions
            SET status = 'yanked', upstream_yanked = TRUE, updated_at = NOW()
            WHERE name = $1 AND version = $2
            ",
        )
        .bind(name)
        .bind(version)
        .execute(&self.pool)
        .await
        .context("marking version yanked (postgres)")?;

        Ok(())
    }

    async fn get_all_quarantined(&self, limit: u32, offset: u32) -> Result<Vec<GemVersion>> {
        let rows = sqlx::query_as::<_, GemVersionRow>(
            r"
            SELECT id, name, version, platform, sha256, published_at, available_after,
                   status, status_reason, upstream_yanked, created_at, updated_at
            FROM gem_versions
            WHERE status = 'quarantine'
            ORDER BY available_after ASC
            LIMIT $1 OFFSET $2
            ",
        )
        .bind(i64::from(limit))
        .bind(i64::from(offset))
        .fetch_all(&self.pool)
        .await
        .context("fetching all quarantined (postgres)")?;

        Ok(into_gem_versions(rows))
    }

    async fn quarantine_stats(&self) -> Result<QuarantineStats> {
        let now = Utc::now();
        let today_end = now + Duration::days(1);
        let week_end = now + Duration::days(7);

        let (quarantined, available, yanked, pinned): (i64, i64, i64, i64) = sqlx::query_as(
            r"
            SELECT
                COALESCE(SUM(CASE WHEN status = 'quarantine' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN status = 'available' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN status = 'yanked' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN status = 'pinned' THEN 1 ELSE 0 END), 0)
            FROM gem_versions
            ",
        )
        .fetch_one(&self.pool)
        .await
        .context("fetching quarantine counts (postgres)")?;

        let releasing_today: i64 = sqlx::query_scalar(
            r"
            SELECT COUNT(*)
            FROM gem_versions
            WHERE status = 'quarantine'
              AND available_after > $1
              AND available_after <= $2
            ",
        )
        .bind(now)
        .bind(today_end)
        .fetch_one(&self.pool)
        .await
        .context("counting versions releasing today (postgres)")?;

        let releasing_week: i64 = sqlx::query_scalar(
            r"
            SELECT COUNT(*)
            FROM gem_versions
            WHERE status = 'quarantine'
              AND available_after > $1
              AND available_after <= $2
            ",
        )
        .bind(now)
        .bind(week_end)
        .fetch_one(&self.pool)
        .await
        .context("counting versions releasing this week (postgres)")?;

        Ok(build_quarantine_stats(
            quarantined,
            available,
            yanked,
            pinned,
            releasing_today,
            releasing_week,
        ))
    }

    async fn get_gem_versions_for_index(&self, name: &str) -> Result<Vec<GemVersion>> {
        let rows = sqlx::query_as::<_, GemVersionRow>(
            r"
            SELECT id, name, version, platform, sha256, published_at, available_after,
                   status, status_reason, upstream_yanked, created_at, updated_at
            FROM gem_versions
            WHERE name = $1
            ",
        )
        .bind(name)
        .fetch_all(&self.pool)
        .await
        .context("fetching gem versions for index (postgres)")?;

        Ok(newest_first(rows, |version: &GemVersion| &version.version))
    }

    async fn insert_symbols(&self, symbol: super::GemSymbolRecord<'_>) -> Result<()> {
        sqlx::query(
            r"
            INSERT INTO gem_symbols (
                gem_name,
                gem_version,
                gem_platform,
                file_path,
                symbol_type,
                symbol_name,
                parent_name,
                line_number
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (gem_name, gem_version, gem_platform, file_path, symbol_name)
            DO UPDATE SET
                symbol_type = EXCLUDED.symbol_type,
                parent_name = EXCLUDED.parent_name,
                line_number = EXCLUDED.line_number
            ",
        )
        .bind(symbol.gem_name)
        .bind(symbol.gem_version)
        .bind(symbol.gem_platform)
        .bind(symbol.file_path)
        .bind(symbol.symbol_type)
        .bind(symbol.symbol_name)
        .bind(symbol.parent_name)
        .bind(symbol.line_number)
        .execute(&self.pool)
        .await
        .context("inserting gem symbol (postgres)")?;

        Ok(())
    }

    async fn clear_symbols(
        &self,
        gem_name: &str,
        gem_version: &str,
        gem_platform: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            r"
            DELETE FROM gem_symbols
            WHERE gem_name = $1
              AND gem_version = $2
              AND gem_platform IS NOT DISTINCT FROM $3
            ",
        )
        .bind(gem_name)
        .bind(gem_version)
        .bind(gem_platform)
        .execute(&self.pool)
        .await
        .context("clearing gem symbols (postgres)")?;

        Ok(())
    }
}

/// Naming and cleanup of the per-test schemas `connect_isolated` creates.
#[cfg(any(test, feature = "test-support"))]
mod test_schema {
    use std::{
        sync::atomic::{AtomicU32, Ordering},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use anyhow::{Context, Result};
    use sqlx::{
        AssertSqlSafe, Connection as _, PgConnection, Row as _, postgres::PgConnectOptions,
    };
    use tokio::sync::OnceCell;

    const PREFIX: &str = "vein_test_";
    /// A schema this old belongs to a run that is no longer alive; a suite
    /// run finishes well within it, and each process only sweeps once.
    const STALE_AFTER: Duration = Duration::from_mins(10);

    static COUNTER: AtomicU32 = AtomicU32::new(0);
    static SWEPT: OnceCell<()> = OnceCell::const_new();

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs())
    }

    /// `vein_test_<unix seconds>_<pid>_<n>`: unique per call, and old enough
    /// runs can be told apart by the timestamp.
    pub(super) fn fresh_name() -> String {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("{PREFIX}{}_{}_{n}", now_secs(), std::process::id())
    }

    /// Drops `schema` when dropped, on its own thread and runtime because
    /// `Drop` is synchronous and the owning test runtime may be shutting down.
    #[derive(Debug)]
    pub(super) struct SchemaGuard {
        options: PgConnectOptions,
        schema: String,
    }

    impl SchemaGuard {
        pub(super) const fn new(options: PgConnectOptions, schema: String) -> Self {
            Self { options, schema }
        }
    }

    impl Drop for SchemaGuard {
        fn drop(&mut self) {
            let options = self.options.clone();
            let schema = std::mem::take(&mut self.schema);
            // Detached and best-effort: a test binary may exit before the
            // drop lands, and `sweep_stale` then catches the leftover.
            std::thread::spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                runtime.block_on(async {
                    if let Ok(mut conn) = PgConnection::connect_with(&options).await {
                        let _ = sqlx::query(AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
                            .execute(&mut conn)
                            .await;
                    }
                });
            });
        }
    }

    fn created_at(schema: &str) -> Option<u64> {
        schema.strip_prefix(PREFIX)?.split('_').next()?.parse().ok()
    }

    /// Drop schemas from runs older than [`STALE_AFTER`], once per process.
    pub(super) async fn sweep_stale(conn: &mut PgConnection) -> Result<()> {
        SWEPT
            .get_or_try_init(|| async {
                let rows = sqlx::query("SELECT nspname FROM pg_namespace WHERE nspname LIKE $1")
                    .bind(format!("{PREFIX}%"))
                    .fetch_all(&mut *conn)
                    .await
                    .context("listing test schemas")?;
                let cutoff = now_secs().saturating_sub(STALE_AFTER.as_secs());
                for row in rows {
                    let schema: String = row.try_get("nspname")?;
                    if created_at(&schema).is_some_and(|created| created < cutoff) {
                        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
                            .execute(&mut *conn)
                            .await
                            .with_context(|| format!("dropping stale test schema {schema}"))?;
                    }
                }
                Ok(())
            })
            .await
            .map(drop)
    }
}
