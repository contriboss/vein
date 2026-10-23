use anyhow::{Context, Result};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
#[cfg(any(test, feature = "test-support"))]
use sqlx::{Connection as _, PgConnection, postgres::PgConnectOptions};
use sqlx::{
    PgPool, Transaction,
    postgres::{PgPoolOptions, Postgres},
};

use crate::version::sort_newest_first;

use super::{
    CacheBackendTrait, PackageVersion, QuarantineStats, VersionStatus,
    backend_common::{
        build_index_stats, build_quarantine_stats, build_sbom_coverage, json_array_like_pattern,
        newest_first, search_like_pattern, search_prefix_pattern,
    },
    models::{
        CachedAssetRow, CachedVersionRow, DbGemMetadataRow, PackageVersionRow, format_timestamp,
    },
    serialization::{hydrate_metadata_row, parse_language_rows, prepare_metadata_strings},
    types::{
        AssetKey, AssetKind, CachedAsset, CachedVersion, CatalogName, GemMetadata, IndexStats,
        SbomCoverage,
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

        let last_accessed: Option<jiff_sqlx::Timestamp> =
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
            last_accessed.map(|ts| format_timestamp(ts.to_jiff())),
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

    async fn names_search(
        &self,
        kind: AssetKind,
        query: &str,
        limit: i64,
    ) -> Result<Vec<CatalogName>> {
        let sql = if kind == AssetKind::Gem {
            concat!(
                "SELECT name, latest_version FROM catalog_gems WHERE name ILIKE $1 ",
                rank_by_name!()
            )
        } else {
            concat!(
                "SELECT name, latest_version FROM registry_names ",
                "WHERE kind = $5 AND name ILIKE $1 ",
                rank_by_name!()
            )
        };
        let mut search = sqlx::query_as::<_, (String, Option<String>)>(sql)
            .bind(search_like_pattern(query))
            .bind(limit)
            .bind(query)
            .bind(search_prefix_pattern(query));
        if kind != AssetKind::Gem {
            search = search.bind(kind.as_str());
        }
        let rows = search
            .fetch_all(&self.pool)
            .await
            .context("searching registry names (postgres)")?;
        Ok(rows
            .into_iter()
            .map(|(name, latest_version)| CatalogName {
                name,
                latest_version,
            })
            .collect())
    }

    async fn names_upsert(&self, kind: AssetKind, names: &[CatalogName]) -> Result<()> {
        if names.is_empty() {
            return Ok(());
        }
        let (names, versions): (Vec<&str>, Vec<Option<&str>>) = names
            .iter()
            .map(|entry| (entry.name.as_str(), entry.latest_version.as_deref()))
            .unzip();
        let sql = if kind == AssetKind::Gem {
            r"
            INSERT INTO catalog_gems (name, latest_version, synced_at)
            SELECT name, version, NOW() FROM UNNEST($1::TEXT[], $2::TEXT[]) AS t(name, version)
            ON CONFLICT (name) DO UPDATE SET
                latest_version = COALESCE(EXCLUDED.latest_version, catalog_gems.latest_version),
                synced_at = EXCLUDED.synced_at
            "
        } else {
            r"
            INSERT INTO registry_names (kind, name, latest_version, synced_at)
            SELECT $3, name, version, NOW() FROM UNNEST($1::TEXT[], $2::TEXT[]) AS t(name, version)
            ON CONFLICT (kind, name) DO UPDATE SET
                latest_version = COALESCE(EXCLUDED.latest_version, registry_names.latest_version),
                synced_at = EXCLUDED.synced_at
            "
        };
        let mut upsert = sqlx::query(sql).bind(&names).bind(&versions);
        if kind != AssetKind::Gem {
            upsert = upsert.bind(kind.as_str());
        }
        upsert
            .execute(&self.pool)
            .await
            .context("upserting registry names (postgres)")?;
        Ok(())
    }

    async fn names_remove(&self, kind: AssetKind, names: &[String]) -> Result<()> {
        if names.is_empty() {
            return Ok(());
        }
        let query = if kind == AssetKind::Gem {
            sqlx::query("DELETE FROM catalog_gems WHERE name = ANY($1)").bind(names)
        } else {
            sqlx::query("DELETE FROM registry_names WHERE kind = $2 AND name = ANY($1)")
                .bind(names)
                .bind(kind.as_str())
        };
        query
            .execute(&self.pool)
            .await
            .context("removing registry names (postgres)")?;
        Ok(())
    }

    async fn names_count(&self, kind: AssetKind) -> Result<u64> {
        let count: i64 = if kind == AssetKind::Gem {
            sqlx::query_scalar("SELECT COUNT(*) FROM catalog_gems")
                .fetch_one(&self.pool)
                .await
        } else {
            sqlx::query_scalar("SELECT COUNT(*) FROM registry_names WHERE kind = $1")
                .bind(kind.as_str())
                .fetch_one(&self.pool)
                .await
        }
        .context("counting registry names (postgres)")?;
        Ok(count.max(0).cast_unsigned())
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

    // ==================== Release delay ====================

    async fn package_versions(&self, kind: AssetKind, name: &str) -> Result<Vec<PackageVersion>> {
        let rows = sqlx::query_as::<_, PackageVersionRow>(
            r"
            SELECT id, kind, name, version, platform, published_at, available_after,
                   status, status_reason, created_at, updated_at
            FROM package_versions
            WHERE kind = $1 AND name = $2
            ",
        )
        .bind(kind.as_str())
        .bind(name)
        .fetch_all(&self.pool)
        .await
        .context("fetching tracked versions (postgres)")?;

        let mut versions = rows
            .into_iter()
            .map(PackageVersion::try_from)
            .collect::<Result<Vec<_>>>()?;
        sort_newest_first(&mut versions, |version: &PackageVersion| &version.version);
        Ok(versions)
    }

    async fn record_held(&self, versions: &[PackageVersion]) -> Result<()> {
        if versions.is_empty() {
            return Ok(());
        }
        let mut tx = self
            .pool
            .begin()
            .await
            .context("starting held versions transaction (postgres)")?;
        for version in versions {
            // An admin's decision (or an earlier release) stands; only the
            // timing of a version still held follows the current policy.
            sqlx::query(
                r"
                INSERT INTO package_versions (
                    kind, name, version, platform, published_at, available_after,
                    status, status_reason
                )
                VALUES ($1, $2, $3, $4, $5, $6, 'quarantine', 'auto')
                ON CONFLICT ON CONSTRAINT package_versions_unique DO UPDATE SET
                    published_at = EXCLUDED.published_at,
                    available_after = EXCLUDED.available_after,
                    updated_at = NOW()
                WHERE package_versions.status = 'quarantine'
                  AND package_versions.available_after <> EXCLUDED.available_after
                ",
            )
            .bind(version.kind.as_str())
            .bind(&version.name)
            .bind(&version.version)
            .bind(version.platform.as_deref())
            .bind(version.published_at.to_sqlx())
            .bind(version.available_after.to_sqlx())
            .execute(&mut *tx)
            .await
            .context("recording held version (postgres)")?;
        }
        tx.commit()
            .await
            .context("committing held versions (postgres)")?;
        Ok(())
    }

    async fn set_version_status(
        &self,
        version: &AssetKey<'_>,
        status: VersionStatus,
        reason: Option<String>,
    ) -> Result<()> {
        // A decision can come before vein ever held the version (blocking
        // a release ahead of time), so it is an upsert.
        sqlx::query(
            r"
            INSERT INTO package_versions (
                kind, name, version, platform, published_at, available_after,
                status, status_reason
            )
            VALUES ($1, $2, $3, $4, NOW(), NOW(), $5, $6)
            ON CONFLICT ON CONSTRAINT package_versions_unique DO UPDATE SET
                status = EXCLUDED.status,
                status_reason = EXCLUDED.status_reason,
                updated_at = NOW()
            ",
        )
        .bind(version.kind.as_str())
        .bind(version.name)
        .bind(version.version)
        .bind(version.platform)
        .bind(status.to_string())
        .bind(reason)
        .execute(&self.pool)
        .await
        .context("setting version status (postgres)")?;

        Ok(())
    }

    async fn promote_expired_quarantines(&self, now: Timestamp) -> Result<u64> {
        let result = sqlx::query(
            r"
            UPDATE package_versions
            SET status = 'available', status_reason = 'auto-promoted', updated_at = NOW()
            WHERE status = 'quarantine'
              AND available_after <= $1
            ",
        )
        .bind(now.to_sqlx())
        .execute(&self.pool)
        .await
        .context("promoting expired quarantines (postgres)")?;

        Ok(result.rows_affected())
    }

    async fn held_versions(&self, limit: u32, offset: u32) -> Result<Vec<PackageVersion>> {
        let rows = sqlx::query_as::<_, PackageVersionRow>(
            r"
            SELECT id, kind, name, version, platform, published_at, available_after,
                   status, status_reason, created_at, updated_at
            FROM package_versions
            WHERE status = 'quarantine'
            ORDER BY available_after ASC, kind, name, version
            LIMIT $1 OFFSET $2
            ",
        )
        .bind(i64::from(limit))
        .bind(i64::from(offset))
        .fetch_all(&self.pool)
        .await
        .context("fetching held versions (postgres)")?;

        rows.into_iter().map(PackageVersion::try_from).collect()
    }

    async fn quarantine_stats(&self) -> Result<QuarantineStats> {
        let now = Timestamp::now();
        let today_end = now
            .checked_add(SignedDuration::from_hours(24))
            .context("a day from now")?;
        let week_end = now
            .checked_add(SignedDuration::from_hours(7 * 24))
            .context("a week from now")?;

        let (quarantined, available, blocked, pinned, releasing_today, releasing_week): (
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
        ) = sqlx::query_as(
            r"
            SELECT
                COUNT(*) FILTER (WHERE status = 'quarantine'),
                COUNT(*) FILTER (WHERE status = 'available'),
                COUNT(*) FILTER (WHERE status = 'blocked'),
                COUNT(*) FILTER (WHERE status = 'pinned'),
                COUNT(*) FILTER (WHERE status = 'quarantine'
                                   AND available_after > $1 AND available_after <= $2),
                COUNT(*) FILTER (WHERE status = 'quarantine'
                                   AND available_after > $1 AND available_after <= $3)
            FROM package_versions
            ",
        )
        .bind(now.to_sqlx())
        .bind(today_end.to_sqlx())
        .bind(week_end.to_sqlx())
        .fetch_one(&self.pool)
        .await
        .context("counting tracked versions (postgres)")?;

        Ok(build_quarantine_stats(
            quarantined,
            available,
            blocked,
            pinned,
            releasing_today,
            releasing_week,
        ))
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
