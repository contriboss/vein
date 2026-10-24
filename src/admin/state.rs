//! Shared state for the admin dashboard.

use std::{path::PathBuf, sync::Arc};

use crate::config::Config as VeinConfig;
use anyhow::{Context, Result};
use jiff::Timestamp;
use rama::http::HeaderMap;
use tera::Tera;
use vein_adapter::{
    AssetKey, AssetKind, CacheBackend, CacheBackendTrait, CachedVersion, FilesystemStorage,
    GemMetadata, IndexStats, PackageVersion, QuarantineStats, SbomCoverage, VersionStatus,
};

use crate::admin::ruby::RubyStatus;

/// Main application state, passed to all handlers.
#[derive(Clone)]
pub struct AdminState {
    pub resources: AdminResources,
    pub tera: Arc<Tera>,
}

impl AdminState {
    #[must_use]
    pub const fn new(resources: AdminResources, tera: Arc<Tera>) -> Self {
        Self { resources, tera }
    }
}

#[derive(Clone)]
pub struct AdminResources {
    config: Arc<VeinConfig>,
    cache: Arc<CacheBackend>,
    storage: Arc<FilesystemStorage>,
    ruby_status: Arc<RubyStatus>,
}

impl AdminResources {
    #[must_use]
    pub const fn new(
        config: Arc<VeinConfig>,
        cache: Arc<CacheBackend>,
        storage: Arc<FilesystemStorage>,
        ruby_status: Arc<RubyStatus>,
    ) -> Self {
        Self {
            config,
            cache,
            storage,
            ruby_status,
        }
    }

    pub async fn snapshot(&self) -> Result<DashboardSnapshot> {
        let index_stats = self.cache.stats().await?;
        let catalog_total = self.cache.catalog_total().await?;

        let upstream = self.config.upstream.as_ref().map(|up| up.url.to_string());

        Ok(DashboardSnapshot {
            generated_at: Timestamp::now(),
            index: index_stats,
            storage_path: self.config.storage.path.clone(),
            database: self.config.database.location(),
            upstream,
            server_host: self.config.server.host.clone(),
            server_port: self.config.server.port,
            worker_count: self.config.server.workers as u64,
            catalog_total,
            ruby_status: self.ruby_status.clone(),
            sbom: self.cache.sbom_coverage().await?,
        })
    }

    pub async fn catalog_total(&self) -> Result<u64> {
        self.cache.catalog_total().await
    }

    pub async fn catalog_page(&self, offset: i64, limit: i64) -> Result<Vec<String>> {
        self.cache.catalog_page(offset, limit).await
    }

    pub async fn catalog_search(&self, query: &str, limit: i64) -> Result<Vec<String>> {
        self.cache.catalog_search(query, limit).await
    }

    pub async fn catalog_page_by_language(
        &self,
        language: &str,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<String>> {
        self.cache
            .catalog_page_by_language(language, offset, limit)
            .await
    }

    pub async fn catalog_total_by_language(&self, language: &str) -> Result<u64> {
        self.cache.catalog_total_by_language(language).await
    }

    /// Cached versions of gem `name`, newest first.
    pub async fn gem_versions(&self, name: &str) -> Result<Vec<String>> {
        self.version_names(AssetKind::Gem, name).await
    }

    /// Versions of `name` cached in `kind`'s ecosystem, newest first, each
    /// listed once however many platform builds it has.
    pub async fn version_names(&self, kind: AssetKind, name: &str) -> Result<Vec<String>> {
        let mut versions: Vec<String> = self
            .cached_versions(kind, name)
            .await?
            .into_iter()
            .map(|cached| cached.version)
            .collect();
        versions.dedup();
        Ok(versions)
    }

    /// Cached artifacts of `name` in `kind`'s ecosystem, newest first.
    pub async fn cached_versions(&self, kind: AssetKind, name: &str) -> Result<Vec<CachedVersion>> {
        self.cache.cached_versions(kind, name).await
    }

    /// Cached `kind` packages whose name contains `query`, best matches first.
    pub async fn search_cached(
        &self,
        kind: AssetKind,
        query: &str,
        limit: i64,
    ) -> Result<Vec<String>> {
        self.cache.search_cached(kind, query, limit).await
    }

    /// Contents of a file in the proxy's storage, or `None` if not cached.
    /// `relative` as stored, else fetched through vein's own proxy at
    /// `proxy_path`, which caches it exactly as any client's request would
    /// (same upstreams, fallbacks and storage). `None` when the registry has
    /// no such package. `npm` marks the request as npm's.
    ///
    /// # Errors
    ///
    /// When storage cannot be read or the proxy cannot be reached.
    pub async fn stored_or_fetched(
        &self,
        relative: &str,
        proxy_path: &str,
        npm: bool,
    ) -> Result<Option<Vec<u8>>> {
        if let Some(bytes) = self.read_stored(relative).await? {
            return Ok(Some(bytes));
        }
        self.fetch_through_proxy(proxy_path, npm).await
    }

    async fn fetch_through_proxy(&self, path: &str, npm: bool) -> Result<Option<Vec<u8>>> {
        Ok(self.proxy_get(path, npm).await?.map(|(_, body)| body))
    }

    /// A search through the proxy's own endpoint, the one `cargo search`
    /// and `npm search` use: the names, best first, and where they came
    /// from (`registry`, `catalog` or `cached`).
    pub async fn search(
        &self,
        kind: AssetKind,
        query: &str,
        limit: i64,
    ) -> Result<(String, Vec<String>)> {
        let text = percent_encoding::utf8_percent_encode(query, percent_encoding::NON_ALPHANUMERIC);
        let path = match kind {
            AssetKind::Gem | AssetKind::Spec => format!("/api/v1/search.json?query={text}"),
            AssetKind::Crate => format!("/api/v1/crates?q={text}&per_page={limit}"),
            AssetKind::NpmPackage => format!("/-/v1/search?text={text}&size={limit}"),
        };
        let (headers, body) = self
            .proxy_get(&path, false)
            .await?
            .with_context(|| format!("the proxy has no search at {path}"))?;
        let source = headers
            .get(crate::search::SOURCE_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("registry")
            .to_owned();
        let answer: serde_json::Value = serde_json::from_slice(&body)
            .with_context(|| format!("reading the search answer for {path}"))?;
        Ok((source, crate::search::names(kind, &answer)))
    }

    /// Package documentation over this cache and proxy.
    #[must_use]
    pub fn docs(&self) -> crate::docs::Docs {
        crate::docs::Docs::new(
            Arc::clone(&self.config),
            Arc::clone(&self.cache),
            Arc::clone(&self.storage),
        )
    }

    async fn proxy_get(&self, path: &str, npm: bool) -> Result<Option<(HeaderMap, Vec<u8>)>> {
        crate::loopback::get(&self.config.server, path, npm).await
    }

    pub async fn read_stored(&self, relative: &str) -> Result<Option<Vec<u8>>> {
        match tokio::fs::read(self.storage.resolve(relative)).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err).with_context(|| format!("reading cached {relative}")),
        }
    }

    pub async fn gem_metadata(
        &self,
        name: &str,
        version: &str,
        platform: Option<&str>,
    ) -> Result<Option<GemMetadata>> {
        self.cache.gem_metadata(name, version, platform).await
    }

    // Quarantine methods
    #[must_use]
    pub fn quarantine_enabled(&self) -> bool {
        self.config.delay_policy.enabled
    }

    pub async fn quarantine_stats(&self) -> Result<QuarantineStats> {
        self.cache.quarantine_stats().await
    }

    /// Versions the delay still holds, soonest released first, across
    /// registries.
    pub async fn quarantine_pending(&self, limit: u32, offset: u32) -> Result<Vec<PackageVersion>> {
        self.cache.held_versions(limit, offset).await
    }

    /// Every version of `name` in registry `kind` the delay policy tracks,
    /// newest first.
    pub async fn tracked_versions(
        &self,
        kind: AssetKind,
        name: &str,
    ) -> Result<Vec<PackageVersion>> {
        self.cache.package_versions(kind, name).await
    }

    /// Releases `version` now, whatever its delay.
    pub async fn approve_version(&self, version: &AssetKey<'_>, reason: &str) -> Result<()> {
        self.cache
            .set_version_status(
                version,
                VersionStatus::Pinned,
                Some(format!("approved: {reason}")),
            )
            .await
    }

    /// Hides `version` from the index and refuses its downloads.
    pub async fn block_version(&self, version: &AssetKey<'_>, reason: &str) -> Result<()> {
        self.cache
            .set_version_status(
                version,
                VersionStatus::Blocked,
                Some(format!("blocked: {reason}")),
            )
            .await
    }
}

pub struct DashboardSnapshot {
    pub generated_at: Timestamp,
    pub index: IndexStats,
    pub storage_path: PathBuf,
    pub database: String,
    pub upstream: Option<String>,
    pub server_host: String,
    pub server_port: u16,
    pub worker_count: u64,
    pub catalog_total: u64,
    pub ruby_status: Arc<RubyStatus>,
    pub sbom: SbomCoverage,
}
