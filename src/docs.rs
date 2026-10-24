//! Package documentation, for people (`GET /docs/...`) and agents (the
//! MCP's `.../readme`): one code path.
//!
//! The README comes from the package's own archive, the one vein caches
//! (fetched through the proxy when nobody did yet), so it is the version's
//! own, not the registry's latest. It is author text: cleaned of what a
//! reader cannot see and capped, like every other `author_supplied` field.

pub mod archive;
pub mod http;
pub mod rdoc;
pub mod rustdoc;
pub mod typings;

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use vein_adapter::{AssetKey, AssetKind, CacheBackend, CacheBackendTrait, FilesystemStorage};

use crate::config::Config;
use crate::{loopback, untrusted};

/// Longest README kept, in characters, before `…(truncated)`.
pub const README_LIMIT: usize = 40_000;
/// Longest API page, in characters.
pub const PAGE_LIMIT: usize = 60_000;

/// One version of one package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub kind: AssetKind,
    pub name: String,
    pub version: String,
    /// Gems only: a native build's platform.
    pub platform: Option<String>,
}

impl Package {
    /// `name` `version` in registry `kind`, if both could be one: they go
    /// into proxy paths, so nothing that could leave one.
    pub fn new(kind: AssetKind, name: &str, version: &str) -> Result<Self> {
        let segment = |part: &str| {
            !part.is_empty()
                && !part.starts_with('.')
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        };
        let name_ok = name.len() <= 214
            && match (kind, name.strip_prefix('@')) {
                (AssetKind::NpmPackage, Some(scoped)) => scoped
                    .split_once('/')
                    .is_some_and(|(scope, rest)| segment(scope) && segment(rest)),
                _ => segment(name),
            };
        let version_ok = version.len() <= 128
            && !version.is_empty()
            && version
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'));
        if !name_ok {
            bail!("`{name}` is not a {} name", registry_noun(kind));
        }
        if !version_ok {
            bail!("`{version}` is not a version");
        }
        // A gem version names its native build: `1.18.10-x86_64-linux-gnu`.
        let (version, platform) = match (kind, version.split_once('-')) {
            (AssetKind::Gem, Some((version, platform))) => (version, Some(platform.to_owned())),
            _ => (version, None),
        };
        Ok(Self {
            kind,
            name: name.to_owned(),
            version: version.to_owned(),
            platform,
        })
    }

    /// `rack 3.2.7`, `nokogiri 1.18.10 (x86_64-linux-gnu)`.
    #[must_use]
    pub fn label(&self) -> String {
        match &self.platform {
            Some(platform) => format!("{} {} ({platform})", self.name, self.version),
            None => format!("{} {}", self.name, self.version),
        }
    }

    /// Where the proxy serves the archive, as a client downloads it.
    fn download_path(&self) -> String {
        match self.kind {
            AssetKind::Gem | AssetKind::Spec => match &self.platform {
                Some(platform) => format!("/gems/{}-{}-{platform}.gem", self.name, self.version),
                None => format!("/gems/{}-{}.gem", self.name, self.version),
            },
            AssetKind::Crate => format!("/api/v1/crates/{}/{}/download", self.name, self.version),
            AssetKind::NpmPackage => {
                let base = self.name.rsplit('/').next().unwrap_or(&self.name);
                format!("/{}/-/{base}-{}.tgz", self.name, self.version)
            }
        }
    }
}

/// A README, cleaned and capped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Readme {
    /// Its file name in the archive (`README.md`, `README.rdoc`).
    pub file: String,
    pub text: String,
    /// Whether it was cut at [`README_LIMIT`].
    pub truncated: bool,
}

/// The registry publishes no such version (or package).
#[derive(Debug)]
pub struct NotPublished(pub String);

impl std::fmt::Display for NotPublished {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} is not published", self.0)
    }
}

impl std::error::Error for NotPublished {}

/// The registry's docs host has no API docs for this version (docs.rs
/// builds before 2025 carry no rustdoc JSON).
#[derive(Debug)]
pub struct NoApiDocs(pub String);

impl std::fmt::Display for NoApiDocs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no API docs for {}", self.0)
    }
}

impl std::error::Error for NoApiDocs {}

/// API docs, as markdown: the outline, or one module or item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Api {
    /// Where they come from: `docs.rs`, the package's `.d.ts`, its Ruby.
    pub source: String,
    pub text: String,
}

/// Documentation lookups over vein's cache and proxy.
#[derive(Clone)]
pub struct Docs {
    config: Arc<Config>,
    index: Arc<CacheBackend>,
    storage: Arc<FilesystemStorage>,
}

impl Docs {
    #[must_use]
    pub const fn new(
        config: Arc<Config>,
        index: Arc<CacheBackend>,
        storage: Arc<FilesystemStorage>,
    ) -> Self {
        Self {
            config,
            index,
            storage,
        }
    }

    /// `[docs] docs_rs`, without a trailing `/`.
    fn docs_rs(&self) -> &str {
        self.config.docs.docs_rs.trim_end_matches('/')
    }

    /// `package`'s API docs at `path` (a module or item: `runtime::Runner`),
    /// or its outline. `Ok(None)`: no such public path.
    pub async fn api(&self, package: &Package, path: Option<&str>) -> Result<Option<Api>> {
        match package.kind {
            AssetKind::Crate => self.crate_api(package, path).await,
            AssetKind::NpmPackage => self.npm_api(package, path).await,
            AssetKind::Gem | AssetKind::Spec => self.gem_api(package, path).await,
        }
    }

    /// A gem's API from its Ruby source, with the `RDoc` comments in it.
    async fn gem_api(&self, package: &Package, path: Option<&str>) -> Result<Option<Api>> {
        let Some(gem) = self.archive(package).await? else {
            return Err(NotPublished(package.label()).into());
        };
        let label = package.label();
        let path = path.map(str::to_owned);
        let rendered = tokio::task::spawn_blocking(move || -> Result<Option<String>> {
            let api = rdoc::Api::parse(&rdoc::sources(&gem)?);
            if api.is_empty() {
                return Err(NoApiDocs(label).into());
            }
            Ok(rdoc::render(&api, path.as_deref(), &label))
        })
        .await
        .context("reading the gem's Ruby source")??;
        Ok(page(rendered, "the gem's Ruby source"))
    }

    async fn crate_api(&self, package: &Package, path: Option<&str>) -> Result<Option<Api>> {
        let zst = self.rustdoc_json(package).await?;
        let path = path.map(str::to_owned);
        let page = tokio::task::spawn_blocking(move || {
            rustdoc::decode(&zst).map(|doc| rustdoc::render(&doc, path.as_deref()))
        })
        .await
        .context("rendering the crate's API")??;
        Ok(self::page(page, "docs.rs"))
    }

    /// An npm package's TypeScript declarations: its own `.d.ts` files, else
    /// `DefinitelyTyped`'s `@types/` package (the newest of the same major).
    async fn npm_api(&self, package: &Package, file: Option<&str>) -> Result<Option<Api>> {
        let own = self.typings(package).await?;
        if !own.files.is_empty() {
            let heading = format!("{}: TypeScript declarations", package.label());
            return Ok(page(typings::render(&own, file, &heading), "the package"));
        }
        if package.name.starts_with("@types/") {
            return Err(NoApiDocs(package.label()).into());
        }
        let types_name = typings::definitely_typed(&package.name);
        let Some(version) = self.types_version(&types_name, &package.version).await? else {
            return Err(NoApiDocs(package.label()).into());
        };
        let types = Package::new(AssetKind::NpmPackage, &types_name, &version)?;
        let declarations = self.typings(&types).await?;
        let heading = format!(
            "{}: TypeScript declarations from {} (DefinitelyTyped)",
            package.label(),
            types.label()
        );
        let source = format!("{} (DefinitelyTyped)", types.label());
        Ok(page(
            typings::render(&declarations, file, &heading),
            &source,
        ))
    }

    async fn typings(&self, package: &Package) -> Result<typings::Typings> {
        let Some(archive) = self.archive(package).await? else {
            return Err(NotPublished(package.label()).into());
        };
        tokio::task::spawn_blocking(move || typings::read(&archive))
            .await
            .context("reading the npm tarball")?
    }

    /// The `@types/` version for `version` of the typed package: the newest
    /// release of the same major, else its latest. `None` without one.
    async fn types_version(&self, types_name: &str, version: &str) -> Result<Option<String>> {
        let path = format!("/{}", types_name.replace('/', "%2f"));
        let Some((_, body)) = loopback::get(&self.config.server, &path, true).await? else {
            return Ok(None);
        };
        let packument: serde_json::Value =
            serde_json::from_slice(&body).context("reading the @types packument")?;
        let mut releases: Vec<&str> = packument
            .get("versions")
            .and_then(serde_json::Value::as_object)
            .into_iter()
            .flat_map(|versions| versions.keys().map(String::as_str))
            .filter(|version| !vein_adapter::is_semver_prerelease(version))
            .collect();
        vein_adapter::sort_newest_first(&mut releases, |version| version);
        let major = version.split('.').next().unwrap_or_default();
        Ok(releases
            .iter()
            .find(|release| release.split('.').next() == Some(major))
            .copied()
            .or_else(|| {
                packument
                    .pointer("/dist-tags/latest")
                    .and_then(serde_json::Value::as_str)
            })
            .map(str::to_owned))
    }

    /// docs.rs's rustdoc JSON for `package`, zstd-compressed: cached in
    /// vein's storage after the first fetch.
    async fn rustdoc_json(&self, package: &Package) -> Result<Vec<u8>> {
        use rama::Layer as _;
        use rama::http::{
            StatusCode, body::util::BodyExt, client::EasyHttpWebClient,
            layer::timeout::TimeoutLayer, service::client::HttpClientExt as _,
        };

        let stored = format!("docs/crates/{}/{}.json.zst", package.name, package.version);
        let local = self.storage.resolve(&stored);
        match tokio::fs::read(&local).await {
            Ok(bytes) => return Ok(bytes),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err).context("reading cached rustdoc JSON"),
        }
        let url = format!(
            "{}/crate/{}/{}/json",
            self.docs_rs(),
            package.name,
            package.version
        );
        let client = TimeoutLayer::new(std::time::Duration::from_secs(60))
            .into_layer(EasyHttpWebClient::default());
        let response = client
            .get(url.as_str())
            .header("user-agent", crate::upstream::UA)
            .send()
            .await
            .map_err(|err| anyhow::anyhow!("asking docs.rs for {}: {err}", package.label()))?;
        match response.status() {
            status if status.is_success() => {}
            StatusCode::NOT_FOUND => return Err(NoApiDocs(package.label()).into()),
            status => bail!("docs.rs answered {status} for {}", package.label()),
        }
        let bytes = response
            .into_body()
            .collect()
            .await
            .context("reading docs.rs's answer")?
            .to_bytes()
            .to_vec();
        if let Some(dir) = local.parent() {
            tokio::fs::create_dir_all(dir)
                .await
                .context("caching rustdoc JSON")?;
        }
        tokio::fs::write(&local, &bytes)
            .await
            .context("caching rustdoc JSON")?;
        Ok(bytes)
    }

    /// `package`'s README: `Ok(None)` when the version has none, an error
    /// when the registry has no such version.
    pub async fn readme(&self, package: &Package) -> Result<Option<Readme>> {
        let Some(archive) = self.archive(package).await? else {
            return Err(NotPublished(package.label()).into());
        };
        let kind = package.kind;
        let found = tokio::task::spawn_blocking(move || match kind {
            AssetKind::Gem | AssetKind::Spec => archive::gem_readme(&archive),
            AssetKind::Crate => archive::crate_readme(&archive),
            AssetKind::NpmPackage => archive::npm_readme(&archive),
        })
        .await
        .context("reading the archive")??;
        Ok(found.and_then(|found| {
            let truncated = found.text.chars().count() > README_LIMIT;
            untrusted::clean(&found.text, README_LIMIT).map(|text| Readme {
                file: untrusted::clean(&found.file, untrusted::SHORT_LIMIT).unwrap_or_default(),
                text,
                truncated,
            })
        }))
    }

    /// The package's archive: the cached file, else through the proxy
    /// (which caches it). `None` when the registry has no such version.
    async fn archive(&self, package: &Package) -> Result<Option<Vec<u8>>> {
        let key = AssetKey {
            kind: package.kind,
            name: &package.name,
            version: &package.version,
            platform: package.platform.as_deref(),
        };
        if let Some(asset) = self.index.get(&key).await? {
            match tokio::fs::read(self.storage.resolve(&asset.path)).await {
                Ok(bytes) => return Ok(Some(bytes)),
                // Indexed but gone from disk: fetch it again.
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(err).context("reading the cached archive"),
            }
        }
        let npm = package.kind == AssetKind::NpmPackage;
        Ok(
            loopback::get(&self.config.server, &package.download_path(), npm)
                .await?
                .map(|(_, body)| body),
        )
    }
}

/// A rendered page, cleaned like other author text, from `source`.
fn page(rendered: Option<String>, source: &str) -> Option<Api> {
    rendered
        .and_then(|text| untrusted::clean(&text, PAGE_LIMIT + 20))
        .map(|text| Api {
            source: source.to_owned(),
            text,
        })
}

/// `text` cut at `limit` characters, saying so.
fn cut(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(limit).collect();
    cut.push_str("…(truncated)");
    cut
}

const fn registry_noun(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Gem | AssetKind::Spec => "gem",
        AssetKind::Crate => "crate",
        AssetKind::NpmPackage => "npm package",
    }
}

#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;
