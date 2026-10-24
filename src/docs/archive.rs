//! READMEs inside package archives: a `.gem` (a tar holding `data.tar.gz`),
//! a `.crate` (`<name>-<version>/` in a tar.gz) and an npm tarball
//! (`package/` in a tar.gz).

use std::io::Read;

use anyhow::{Context, Result};
use flate2::read::GzDecoder;

/// The most read from any one file; a README past it is cut there.
const FILE_LIMIT: u64 = 1 << 20;

/// README file names, best first, compared without case.
const README_NAMES: &[&str] = &[
    "readme.md",
    "readme.markdown",
    "readme.rdoc",
    "readme.txt",
    "readme.rst",
    "readme",
];

/// A README: its file name in the archive and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub file: String,
    pub text: String,
}

/// The README of a `.gem`: at the root of its `data.tar.gz`.
pub fn gem_readme(gem: &[u8]) -> Result<Option<Found>> {
    let mut outer = tar::Archive::new(gem);
    for entry in outer.entries().context("reading the .gem")? {
        let entry = entry.context("reading the .gem")?;
        if entry.path().context("reading the .gem")?.as_os_str() == "data.tar.gz" {
            return root_readme(GzDecoder::new(entry), "", None);
        }
    }
    Ok(None)
}

/// The README of a `.crate`: the file its `Cargo.toml` names (published
/// crates always say), else the best `README*` at the root.
pub fn crate_readme(krate: &[u8]) -> Result<Option<Found>> {
    match cargo_readme(krate)? {
        Declared::Path(path) => root_readme(GzDecoder::new(krate), "*", Some(&path)),
        Declared::Unsaid => root_readme(GzDecoder::new(krate), "*", None),
        Declared::Nothing => Ok(None),
    }
}

/// What a crate's `Cargo.toml` says of its README.
enum Declared {
    /// `readme = "docs/OVERVIEW.md"`.
    Path(String),
    /// `readme = false`: it has none.
    Nothing,
    /// No `Cargo.toml`, or no `readme` key.
    Unsaid,
}

/// The README of an npm tarball, at its root (`package/`, usually).
pub fn npm_readme(tarball: &[u8]) -> Result<Option<Found>> {
    root_readme(GzDecoder::new(tarball), "*", None)
}

/// `Cargo.toml`'s `package.readme`.
fn cargo_readme(krate: &[u8]) -> Result<Declared> {
    let mut archive = tar::Archive::new(GzDecoder::new(krate));
    for entry in archive.entries().context("reading the .crate")? {
        let entry = entry.context("reading the .crate")?;
        let path = entry.path().context("reading the .crate")?.into_owned();
        let mut parts = path.components();
        let (Some(_root), Some(file), None) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if file.as_os_str() != "Cargo.toml" {
            continue;
        }
        let manifest: toml::Table = read_text(entry)?
            .parse()
            .context("reading the crate's Cargo.toml")?;
        let readme = manifest
            .get("package")
            .and_then(|package| package.get("readme"));
        return Ok(match readme {
            Some(toml::Value::String(path)) => Declared::Path(path.clone()),
            Some(toml::Value::Boolean(false)) => Declared::Nothing,
            _ => Declared::Unsaid,
        });
    }
    Ok(Declared::Unsaid)
}

/// The README at the archive's root: `root` is `""` for an archive whose
/// files sit at the top, `"*"` for one under a single directory. With
/// `wanted`, that path (relative to the root) only.
fn root_readme(tar_gz: impl Read, root: &str, wanted: Option<&str>) -> Result<Option<Found>> {
    let mut archive = tar::Archive::new(tar_gz);
    let mut best: Option<(usize, Found)> = None;
    for entry in archive.entries().context("reading the archive")? {
        let entry = entry.context("reading the archive")?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().context("reading the archive")?.into_owned();
        let Some(relative) = under_root(&path, root) else {
            continue;
        };
        let rank = match wanted {
            Some(wanted) => (normalize(&relative) == normalize(wanted)).then_some(0),
            None => README_NAMES
                .iter()
                .position(|name| relative.eq_ignore_ascii_case(name)),
        };
        let Some(rank) = rank else { continue };
        if best.as_ref().is_some_and(|(seen, _)| *seen <= rank) {
            continue;
        }
        let text = read_text(entry)?;
        best = Some((
            rank,
            Found {
                file: relative,
                text,
            },
        ));
        if rank == 0 {
            break;
        }
    }
    Ok(best.map(|(_, found)| found))
}

/// `path` below the archive's root, if it is a file there (not deeper,
/// unless the wanted path says so).
fn under_root(path: &std::path::Path, root: &str) -> Option<String> {
    let text = path.to_str()?;
    let relative = if root.is_empty() {
        text
    } else {
        text.split_once('/')?.1
    };
    (!relative.is_empty()).then(|| relative.to_owned())
}

/// `./README.md` and `README.md` are the same file.
fn normalize(path: &str) -> &str {
    path.trim_start_matches("./")
}

pub(super) fn read_text(entry: impl Read) -> Result<String> {
    let mut bytes = Vec::new();
    entry
        .take(FILE_LIMIT)
        .read_to_end(&mut bytes)
        .context("reading a file in the archive")?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests;
