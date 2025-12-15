use std::time::Instant;

use anyhow::{Context, Result};
use rama::http::{Body, Method, Request};
use reqwest::Url;
use vein_adapter::{AssetKey, AssetKind};

/// Cache status for request tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheStatus {
    Pass,
    Hit,
    Miss,
    Revalidated,
    Error,
}

impl std::fmt::Display for CacheStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheStatus::Pass => write!(f, "pass"),
            CacheStatus::Hit => write!(f, "hit"),
            CacheStatus::Miss => write!(f, "miss"),
            CacheStatus::Revalidated => write!(f, "revalidated"),
            CacheStatus::Error => write!(f, "error"),
        }
    }
}

/// Request context for tracking request lifecycle
pub struct RequestContext {
    pub start: Instant,
    pub method: Method,
    pub path: String,
    pub cache: CacheStatus,
}

impl Default for RequestContext {
    fn default() -> Self {
        Self {
            start: Instant::now(),
            method: Method::GET,
            path: String::new(),
            cache: CacheStatus::Pass,
        }
    }
}

impl RequestContext {
    pub fn from_request(req: &Request<Body>) -> Self {
        Self {
            start: Instant::now(),
            method: req.method().clone(),
            path: req.uri().path().to_string(),
            cache: CacheStatus::Pass,
        }
    }
}

/// Represents a cacheable gem or spec request
pub struct CacheableRequest {
    pub kind: AssetKind,
    pub name: String,
    pub version: String,
    pub platform: Option<String>,
    pub file_name: String,
    pub relative_path: String,
}

impl CacheableRequest {
    pub fn from_request(req: &Request<rama::http::Body>) -> Option<Self> {
        let path = req.uri().path();
        if path.starts_with("/gems/") {
            Self::from_gem_path(path.strip_prefix("/gems/")?)
        } else if path.starts_with("/quick/Marshal.4.8/") {
            Self::from_spec_path(path.strip_prefix("/quick/Marshal.4.8/")?)
        } else {
            None
        }
    }

    pub fn from_gem_path(file: &str) -> Option<Self> {
        if !file.ends_with(".gem") {
            return None;
        }
        // Reject path traversal attempts
        if file.contains("..") || file.contains("//") || file.starts_with('/') {
            tracing::warn!(file = %file, "Rejected potential path traversal attempt");
            return None;
        }
        let file_name = file.to_string();
        let stem = file.strip_suffix(".gem")?;
        let (name, version, platform) = super::utils::split_name_version_platform(stem)?;
        // Double check the parsed name doesn't contain path traversal
        if name.contains("..") || name.contains('/') {
            tracing::warn!(name = %name, "Rejected malformed gem name");
            return None;
        }
        let relative_path = format!("gems/{name}/{file}");
        Some(Self {
            kind: AssetKind::Gem,
            name,
            version,
            platform,
            file_name,
            relative_path,
        })
    }

    pub fn from_spec_path(file: &str) -> Option<Self> {
        if !file.ends_with(".gemspec.rz") {
            return None;
        }
        // Reject path traversal attempts
        if file.contains("..") || file.contains("//") || file.starts_with('/') {
            tracing::warn!(file = %file, "Rejected potential path traversal attempt in spec");
            return None;
        }
        let file_name = file.to_string();
        let stem = file.strip_suffix(".gemspec.rz")?;
        let (name, version, platform) = super::utils::split_name_version_platform(stem)?;
        // Double check the parsed name doesn't contain path traversal
        if name.contains("..") || name.contains('/') {
            tracing::warn!(name = %name, "Rejected malformed gem name in spec");
            return None;
        }
        let relative_path = format!("quick/Marshal.4.8/{name}/{file}");
        Some(Self {
            kind: AssetKind::Spec,
            name,
            version,
            platform,
            file_name,
            relative_path,
        })
    }

    pub fn asset_key(&self) -> AssetKey<'_> {
        AssetKey {
            kind: self.kind,
            name: &self.name,
            version: &self.version,
            platform: self.platform.as_deref(),
        }
    }

    pub fn download_name(&self) -> &str {
        &self.file_name
    }

    pub fn content_type(&self) -> &'static str {
        match self.kind {
            AssetKind::Gem => "application/octet-stream",
            AssetKind::Spec => "application/x-deflate",
        }
    }
}

/// Upstream target configuration
#[derive(Clone)]
pub struct UpstreamTarget {
    pub base: Url,
}

impl UpstreamTarget {
    pub fn from_url(url: &Url) -> Result<Self> {
        Ok(Self { base: url.clone() })
    }

    pub fn join(&self, req: &Request<rama::http::Body>) -> Result<Url> {
        let path_and_query = req
            .uri()
            .path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or("/");
        let cleaned = path_and_query.trim_start_matches('/');
        self.base
            .join(cleaned)
            .with_context(|| format!("joining upstream path {path_and_query}"))
    }
}
