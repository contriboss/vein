use std::collections::HashSet;

use anyhow::{Context, Result, anyhow};
use rama::http::{Uri, body::util::BodyExt, header::HeaderMap};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tracing::{error, info, warn};
use vein::{gem_metadata::extract_gem_metadata, upstream::UpstreamClient};
use vein_adapter::{AssetKey, AssetKind, CacheBackend, CacheBackendTrait, FilesystemStorage};

use crate::commands::AppContext;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GemSyncTarget {
    version: String,
    platform: Option<String>,
}

struct SyncEnvironment<'a> {
    client: &'a UpstreamClient,
    cache: &'a CacheBackend,
    storage: &'a FilesystemStorage,
    base_url: Uri,
}

impl GemSyncTarget {
    fn new(version: impl Into<String>, platform: Option<&str>) -> Self {
        Self {
            version: version.into(),
            platform: normalize_platform(platform),
        }
    }

    fn asset_key<'a>(&'a self, name: &'a str) -> AssetKey<'a> {
        AssetKey {
            kind: AssetKind::Gem,
            name,
            version: &self.version,
            platform: self.platform(),
        }
    }

    fn file_name(&self, name: &str) -> String {
        match self.platform() {
            Some(platform) => format!("{name}-{}-{platform}.gem", self.version),
            None => format!("{name}-{}.gem", self.version),
        }
    }

    fn relative_path(&self, name: &str) -> String {
        format!("gems/{name}/{}", self.file_name(name))
    }

    fn platform(&self) -> Option<&str> {
        self.platform.as_deref()
    }
}

pub async fn run(
    vein_config_path: &str,
    name: String,
    version: Option<String>,
    platform: Option<String>,
) -> Result<()> {
    let ctx = AppContext::load(vein_config_path).await?;

    let upstream_config = ctx
        .config
        .upstream
        .as_ref()
        .ok_or_else(|| anyhow!("No upstream configured - cannot sync"))?;

    info!(upstream = %upstream_config.url, "Connecting to upstream");

    let client = UpstreamClient::new(upstream_config).context("creating upstream client")?;
    let storage = FilesystemStorage::new(ctx.config.storage.path.clone());
    storage.prepare().await.context("preparing storage")?;

    let env = SyncEnvironment {
        client: &client,
        cache: ctx.cache.as_ref(),
        storage: &storage,
        base_url: upstream_config.url.clone(),
    };

    let targets =
        resolve_sync_targets(&env, &name, version.as_deref(), platform.as_deref()).await?;

    if targets.is_empty() {
        error!(gem = %name, platform = ?platform, "No matching versions found");
        return Err(anyhow!("No versions found for gem '{}'", name));
    }

    if version.is_some() {
        let target = &targets[0];
        info!(
            gem = %name,
            version = %target.version,
            platform = ?target.platform(),
            "Syncing gem"
        );
        sync_gem(&env, &name, target).await?;
        info!(
            gem = %name,
            version = %target.version,
            platform = ?target.platform(),
            "✓ Synced successfully"
        );
        return Ok(());
    }

    info!(gem = %name, count = targets.len(), "Found versions");

    for target in &targets {
        info!(
            gem = %name,
            version = %target.version,
            platform = ?target.platform(),
            "Syncing"
        );
        match sync_gem(&env, &name, target).await {
            Ok(_) => info!(
                gem = %name,
                version = %target.version,
                platform = ?target.platform(),
                "✓ Synced"
            ),
            Err(e) => error!(
                gem = %name,
                version = %target.version,
                platform = ?target.platform(),
                error = %e,
                "✗ Failed to sync"
            ),
        }
    }

    info!(gem = %name, total = targets.len(), "✓ Sync complete");
    Ok(())
}

async fn sync_gem(env: &SyncEnvironment<'_>, name: &str, target: &GemSyncTarget) -> Result<()> {
    let asset_key = target.asset_key(name);

    if env.cache.get(&asset_key).await?.is_some() {
        info!(
            gem = %name,
            version = %target.version,
            platform = ?target.platform(),
            "Already cached, skipping"
        );
        return Ok(());
    }

    let body_bytes = fetch_gem_bytes(env.client, &env.base_url, name, target).await?;
    let sha_hex = sha256_hex(&body_bytes);
    let relative_path = target.relative_path(name);
    let final_path = write_gem_to_storage(env.storage, &relative_path, &body_bytes).await?;

    env.cache
        .insert_or_replace(
            &asset_key,
            &relative_path,
            &sha_hex,
            body_bytes.len() as u64,
        )
        .await
        .context("storing in cache index")?;

    info!(
        gem = %name,
        version = %target.version,
        platform = ?target.platform(),
        size = body_bytes.len(),
        sha256 = &sha_hex[..8],
        "Cached successfully"
    );

    store_gem_metadata(
        env.cache,
        &final_path,
        name,
        target,
        body_bytes.len() as u64,
        &sha_hex,
    )
    .await;

    Ok(())
}

async fn resolve_sync_targets(
    env: &SyncEnvironment<'_>,
    name: &str,
    version: Option<&str>,
    platform: Option<&str>,
) -> Result<Vec<GemSyncTarget>> {
    if let Some(version) = version {
        return Ok(vec![GemSyncTarget::new(version.to_string(), platform)]);
    }

    info!(gem = %name, "Fetching versions");

    let targets = fetch_gem_targets(env.client, &env.base_url, name).await?;
    Ok(filter_targets(targets, platform))
}

async fn fetch_gem_targets(
    client: &UpstreamClient,
    base_url: &Uri,
    name: &str,
) -> Result<Vec<GemSyncTarget>> {
    let info_path = format!("/info/{name}");
    let url = build_url(base_url, &info_path)?;
    let content = fetch_upstream_text(client, url, "fetching gem info from upstream")
        .await
        .context("fetching gem info from upstream")?;

    Ok(parse_compact_index_targets(&content))
}

async fn fetch_gem_bytes(
    client: &UpstreamClient,
    base_url: &Uri,
    name: &str,
    target: &GemSyncTarget,
) -> Result<Vec<u8>> {
    let gem_path = format!("/gems/{}", target.file_name(name));
    let url = build_url(base_url, &gem_path)?;

    info!(url = %url, "Fetching from upstream");

    fetch_upstream_bytes(client, url, "fetching gem from upstream").await
}

async fn fetch_upstream_text(client: &UpstreamClient, url: Uri, context: &str) -> Result<String> {
    let body_bytes = fetch_upstream_bytes(client, url, context).await?;
    String::from_utf8(body_bytes).context("upstream response is not valid UTF-8")
}

async fn fetch_upstream_bytes(client: &UpstreamClient, url: Uri, context: &str) -> Result<Vec<u8>> {
    let headers = HeaderMap::new();
    let response = client
        .get_with_headers(url, &headers)
        .await
        .with_context(|| context.to_string())?;

    let status = response.status();
    if !status.is_success() {
        return Err(anyhow!(
            "upstream returned {}: {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or("unknown")
        ));
    }

    let body_bytes = response
        .into_body()
        .collect()
        .await
        .context("reading response body")?
        .to_bytes();

    Ok(body_bytes.to_vec())
}

async fn write_gem_to_storage(
    storage: &FilesystemStorage,
    relative_path: &str,
    body_bytes: &[u8],
) -> Result<std::path::PathBuf> {
    let mut temp_file = storage
        .create_temp_writer(relative_path)
        .await
        .context("creating temp file")?;

    temp_file
        .file_mut()
        .write_all(body_bytes)
        .await
        .context("writing gem file")?;

    temp_file.commit().await.context("committing gem file")?;
    Ok(storage.resolve(relative_path))
}

async fn store_gem_metadata(
    cache: &CacheBackend,
    final_path: &std::path::Path,
    name: &str,
    target: &GemSyncTarget,
    size_bytes: u64,
    sha_hex: &str,
) {
    match extract_gem_metadata(
        final_path,
        name,
        &target.version,
        target.platform(),
        size_bytes,
        sha_hex,
        None,
    )
    .await
    {
        Ok(Some(metadata)) => {
            if let Err(e) = cache.upsert_metadata(&metadata).await {
                warn!(
                    gem = %name,
                    version = %target.version,
                    platform = ?target.platform(),
                    error = %e,
                    "Failed to store metadata"
                );
            } else {
                info!(
                    gem = %name,
                    version = %target.version,
                    platform = ?target.platform(),
                    "Metadata extracted and stored"
                );
            }
        }
        Ok(None) => {
            warn!(
                gem = %name,
                version = %target.version,
                platform = ?target.platform(),
                "No metadata found in gem"
            );
        }
        Err(e) => {
            warn!(
                gem = %name,
                version = %target.version,
                platform = ?target.platform(),
                error = %e,
                "Failed to extract metadata"
            );
        }
    }
}

fn parse_compact_index_targets(content: &str) -> Vec<GemSyncTarget> {
    dedupe_targets(content.lines().filter_map(parse_compact_index_line))
}

fn parse_compact_index_line(line: &str) -> Option<GemSyncTarget> {
    let header = line.split('|').next()?.trim();
    if header.is_empty() || header == "---" {
        return None;
    }

    let mut parts = header.split_whitespace();
    let version = parts.next()?.trim();
    if version.is_empty() {
        return None;
    }

    Some(GemSyncTarget::new(version.to_string(), parts.next()))
}

fn filter_targets(targets: Vec<GemSyncTarget>, platform: Option<&str>) -> Vec<GemSyncTarget> {
    let requested_platform = platform
        .map(str::trim)
        .filter(|platform| !platform.is_empty());
    let filtered = targets
        .into_iter()
        .filter(|target| match requested_platform {
            Some("ruby") => target.platform().is_none(),
            Some(platform) => target.platform() == Some(platform),
            None => true,
        });

    dedupe_targets(filtered)
}

fn dedupe_targets(targets: impl IntoIterator<Item = GemSyncTarget>) -> Vec<GemSyncTarget> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();

    for target in targets {
        if seen.insert(target.clone()) {
            deduped.push(target);
        }
    }

    deduped
}

fn normalize_platform(platform: Option<&str>) -> Option<String> {
    platform
        .map(str::trim)
        .filter(|platform| !platform.is_empty() && *platform != "ruby")
        .map(str::to_string)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn build_url(base: &Uri, path: &str) -> Result<Uri> {
    let base_path = base
        .path_and_query()
        .map(|pq| pq.path())
        .unwrap_or("/")
        .trim_end_matches('/');

    let combined = if base_path.is_empty() || base_path == "/" {
        path.to_string()
    } else {
        format!("{base_path}{path}")
    };

    let mut parts = base.clone().into_parts();
    parts.path_and_query = Some(combined.parse().context("parsing combined path")?);

    Uri::from_parts(parts).context("building URL")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_compact_index_targets_parses_platform_variants() {
        let targets = parse_compact_index_targets(
            "---\n1.0.0 ruby|abc123|deps\n1.0.0 x86_64-linux|def456|deps\n2.0.0 |ghi789|deps\n",
        );

        assert_eq!(
            targets,
            vec![
                GemSyncTarget::new("1.0.0", None),
                GemSyncTarget::new("1.0.0", Some("x86_64-linux")),
                GemSyncTarget::new("2.0.0", None),
            ]
        );
    }

    #[test]
    fn filter_targets_keeps_all_variants_when_platform_not_requested() {
        let targets = vec![
            GemSyncTarget::new("1.0.0", None),
            GemSyncTarget::new("1.0.0", Some("x86_64-linux")),
            GemSyncTarget::new("1.0.0", Some("x86_64-linux")),
        ];

        assert_eq!(
            filter_targets(targets, None),
            vec![
                GemSyncTarget::new("1.0.0", None),
                GemSyncTarget::new("1.0.0", Some("x86_64-linux")),
            ]
        );
    }

    #[test]
    fn filter_targets_matches_ruby_platform_to_default_artifact() {
        let targets = vec![
            GemSyncTarget::new("1.0.0", None),
            GemSyncTarget::new("1.0.0", Some("java")),
        ];

        assert_eq!(
            filter_targets(targets, Some("ruby")),
            vec![GemSyncTarget::new("1.0.0", None)]
        );
    }

    #[test]
    fn build_url_preserves_base_path() {
        let base: Uri = "https://mirror.example.com/rubygems/".parse().unwrap();
        let url = build_url(&base, "/info/rack").unwrap();

        assert_eq!(
            url.to_string(),
            "https://mirror.example.com/rubygems/info/rack"
        );
    }
}
