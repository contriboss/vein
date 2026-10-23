//! Shared state for the admin dashboard.

use std::{path::PathBuf, sync::Arc, time::Duration};

use crate::config::Config as VeinConfig;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use rama::{
    Layer,
    http::{
        StatusCode, body::util::BodyExt, client::EasyHttpWebClient, layer::timeout::TimeoutLayer,
        service::client::HttpClientExt,
    },
};
use tera::Tera;
use vein_adapter::{
    AssetKind, CacheBackend, CacheBackendTrait, CachedVersion, FilesystemStorage, GemMetadata,
    GemVersion, IndexStats, QuarantineStats, SbomCoverage, VersionStatus,
};

use crate::admin::ruby::RubyStatus;

/// How long a registry lookup through the proxy may take: its upstream
/// timeout is 30s at most, plus the fallbacks.
const PROXY_TIMEOUT: Duration = Duration::from_secs(60);

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
            generated_at: Utc::now(),
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
        let url = format!("{}{path}", self.proxy_base());
        let client = TimeoutLayer::new(PROXY_TIMEOUT).into_layer(EasyHttpWebClient::default());
        let mut request = client.get(url.as_str());
        if npm {
            request = request.header("npm-command", "view");
        }
        let response = request
            .send()
            .await
            .map_err(|err| anyhow::anyhow!("asking the proxy for {path}: {err}"))?;
        match response.status() {
            status if status.is_success() => {
                let body = response
                    .into_body()
                    .collect()
                    .await
                    .with_context(|| format!("reading the proxy's answer for {path}"))?;
                Ok(Some(body.to_bytes().to_vec()))
            }
            StatusCode::NOT_FOUND => Ok(None),
            status => bail!("the proxy answered {status} for {path}"),
        }
    }

    /// The proxy listener, as this process can reach it.
    fn proxy_base(&self) -> String {
        let server = &self.config.server;
        let host = match server.host.as_str() {
            "" | "0.0.0.0" => "127.0.0.1",
            "::" | "[::]" => "[::1]",
            host => host,
        };
        format!("http://{host}:{}", server.port)
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

    pub async fn quarantine_pending(&self, limit: u32, offset: u32) -> Result<Vec<GemVersion>> {
        self.cache.get_all_quarantined(limit, offset).await
    }

    /// Every version of `name` the quarantine system tracks, newest first.
    pub async fn tracked_versions(&self, name: &str) -> Result<Vec<GemVersion>> {
        self.cache.get_gem_versions_for_index(name).await
    }

    pub async fn approve_version(
        &self,
        name: &str,
        version: &str,
        platform: Option<&str>,
        reason: &str,
    ) -> Result<()> {
        self.cache
            .update_version_status(
                name,
                version,
                platform,
                VersionStatus::Pinned,
                Some(format!("approved: {reason}")),
            )
            .await
    }

    pub async fn block_version(
        &self,
        name: &str,
        version: &str,
        platform: Option<&str>,
        reason: &str,
    ) -> Result<()> {
        self.cache
            .update_version_status(
                name,
                version,
                platform,
                VersionStatus::Yanked,
                Some(format!("blocked: {reason}")),
            )
            .await
    }
}

pub struct DashboardSnapshot {
    pub generated_at: DateTime<Utc>,
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
