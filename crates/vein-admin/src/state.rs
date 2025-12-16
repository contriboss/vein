use std::{path::PathBuf, sync::Arc};

use crate::ruby::RubyStatus;
use anyhow::Result;
use chrono::{DateTime, Utc};
use vein::config::Config as VeinConfig;
use vein_adapter::{
    CacheBackend, GemMetadata, GemVersion, IndexStats, QuarantineStats, SbomCoverage,
    VersionStatus,
};

#[derive(Clone)]
pub struct AdminResources {
    config: Arc<VeinConfig>,
    cache: Arc<dyn CacheBackend>,
    ruby_status: Arc<RubyStatus>,
}

impl AdminResources {
    pub fn new(
        config: Arc<VeinConfig>,
        cache: Arc<dyn CacheBackend>,
        ruby_status: Arc<RubyStatus>,
    ) -> Self {
        Self {
            config,
            cache,
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
            database_path: self.config.database.path.clone(),
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

    pub async fn catalog_languages(&self) -> Result<Vec<String>> {
        self.cache.catalog_languages().await
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

    pub async fn gem_versions(&self, name: &str) -> Result<Vec<String>> {
        let mut versions: Vec<String> = self
            .cache
            .get_all_gems()
            .await?
            .into_iter()
            .filter_map(|(gem, version)| (gem == name).then_some(version))
            .collect();

        versions.sort();
        versions.dedup();
        versions.reverse();
        Ok(versions)
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
    pub fn quarantine_enabled(&self) -> bool {
        self.config.delay_policy.enabled
    }

    pub async fn quarantine_stats(&self) -> Result<QuarantineStats> {
        self.cache.quarantine_stats().await
    }

    pub async fn quarantine_pending(&self, limit: u32, offset: u32) -> Result<Vec<GemVersion>> {
        self.cache.get_all_quarantined(limit, offset).await
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
                Some(format!("approved: {}", reason)),
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
                Some(format!("blocked: {}", reason)),
            )
            .await
    }

    pub async fn get_gem_version(
        &self,
        name: &str,
        version: &str,
        platform: Option<&str>,
    ) -> Result<Option<GemVersion>> {
        self.cache.get_gem_version(name, version, platform).await
    }
}

pub struct DashboardSnapshot {
    pub generated_at: DateTime<Utc>,
    pub index: IndexStats,
    pub storage_path: PathBuf,
    pub database_path: PathBuf,
    pub upstream: Option<String>,
    pub server_host: String,
    pub server_port: u16,
    pub worker_count: u64,
    pub catalog_total: u64,
    pub ruby_status: Arc<RubyStatus>,
    pub sbom: SbomCoverage,
}
