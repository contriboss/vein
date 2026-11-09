//! The registry packages a lockfile pins: `Cargo.lock`, `Gemfile.lock` or
//! `package-lock.json`, told apart by their contents. Path, git and
//! workspace members are left out: no registry publishes them.

use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;
use vein_adapter::AssetKind;

/// One pinned package.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pinned {
    pub name: String,
    pub version: String,
}

impl Pinned {
    fn new(name: &str, version: &str) -> Self {
        Self {
            name: name.to_owned(),
            version: version.to_owned(),
        }
    }
}

/// The registry `text` pins packages of, and those packages, sorted and
/// without repeats.
pub fn read(text: &str) -> Result<(AssetKind, Vec<Pinned>)> {
    let trimmed = text.trim_start();
    let (kind, pinned) = if trimmed.starts_with('{') {
        (AssetKind::NpmPackage, package_lock(text)?)
    } else if text.lines().any(|line| line.trim_end() == "GEM") {
        (AssetKind::Gem, gemfile_lock(text))
    } else if text.contains("[[package]]") {
        (AssetKind::Crate, cargo_lock(text)?)
    } else {
        bail!("not a Cargo.lock, Gemfile.lock or package-lock.json");
    };
    let unique: BTreeSet<Pinned> = pinned.into_iter().collect();
    Ok((kind, unique.into_iter().collect()))
}

#[derive(Deserialize)]
struct CargoLock {
    #[serde(default)]
    package: Vec<CargoPackage>,
}

#[derive(Deserialize)]
struct CargoPackage {
    name: String,
    version: String,
    source: Option<String>,
}

/// Packages from a registry (`registry+...`, `sparse+...`); workspace
/// members have no source, git dependencies a `git+` one.
fn cargo_lock(text: &str) -> Result<Vec<Pinned>> {
    let lock: CargoLock = toml::from_str(text).context("reading Cargo.lock")?;
    Ok(lock
        .package
        .into_iter()
        .filter(|package| {
            package.source.as_deref().is_some_and(|source| {
                source.starts_with("registry+") || source.starts_with("sparse+")
            })
        })
        .map(|package| Pinned {
            name: package.name,
            version: package.version,
        })
        .collect())
}

/// The `specs:` of every `GEM` section: `    name (version)`, a native
/// build's platform (`1.16.0-x86_64-linux`) dropped. `GIT` and `PATH`
/// sections are not rubygems.org's.
fn gemfile_lock(text: &str) -> Vec<Pinned> {
    let mut pinned = Vec::new();
    let mut in_gem = false;
    for line in text.lines() {
        if !line.starts_with(' ') {
            in_gem = line.trim_end() == "GEM";
            continue;
        }
        let Some(spec) = line.strip_prefix("    ").filter(|_| in_gem) else {
            continue;
        };
        if spec.starts_with(' ') {
            continue; // a dependency of the spec above
        }
        let Some((name, rest)) = spec.split_once(" (") else {
            continue;
        };
        let version = rest.trim_end_matches(')');
        let version = version
            .split_once('-')
            .map_or(version, |(version, _)| version);
        pinned.push(Pinned::new(name, version));
    }
    pinned
}

/// `packages` (lockfile v2 and v3: `node_modules/...` paths) or
/// `dependencies` (v1, nested). Linked workspace packages are left out.
fn package_lock(text: &str) -> Result<Vec<Pinned>> {
    let lock: Value = serde_json::from_str(text).context("reading package-lock.json")?;
    let mut pinned = Vec::new();
    if let Some(packages) = lock.get("packages").and_then(Value::as_object) {
        for (path, package) in packages {
            let Some((_, name)) = path.rsplit_once("node_modules/") else {
                continue; // the root project, or a workspace folder
            };
            if package.get("link").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            if let Some(version) = package.get("version").and_then(Value::as_str) {
                pinned.push(Pinned::new(name, version));
            }
        }
    } else if let Some(dependencies) = lock.get("dependencies") {
        nested(dependencies, &mut pinned);
    }
    Ok(pinned)
}

fn nested(dependencies: &Value, pinned: &mut Vec<Pinned>) {
    let Some(dependencies) = dependencies.as_object() else {
        return;
    };
    for (name, package) in dependencies {
        let version = package.get("version").and_then(Value::as_str);
        // `file:` and `link:` versions are local folders.
        if let Some(version) = version.filter(|version| !version.contains(':')) {
            pinned.push(Pinned::new(name, version));
        }
        if let Some(inner) = package.get("dependencies") {
            nested(inner, pinned);
        }
    }
}

#[cfg(test)]
mod tests;
