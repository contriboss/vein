//! Keeping the advisories table in step with the advisory database: a
//! shallow, sparse clone holding only the reviewed advisories, fetched and
//! diffed against the commit last imported. The first run, or one whose
//! last commit is gone from the clone, imports every file instead.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rama::telemetry::tracing::{info, warn};
use tokio::process::Command;
use vein_adapter::{Advisory, AdvisoryImport, CacheBackend};

/// The stampeded task that syncs the advisories.
pub const TASK: &str = "advisories.sync";
/// `advisory_sync`'s row for GitHub's database.
pub const SOURCE: &str = "github";
/// Advisories GitHub reviewed; `unreviewed/` is left out.
const REVIEWED: &str = "advisories/github-reviewed";

/// What one import found, for the log.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Synced {
    pub revision: String,
    pub full: bool,
    pub upserted: usize,
    pub removed: usize,
}

/// Brings `checkout` (cloned from `repository` the first time) up to date
/// and imports what changed since the last revision; `None` when nothing
/// did, or `stop` said so before the import.
pub async fn sync(
    index: &CacheBackend,
    checkout: &Path,
    repository: &str,
    stop: &(dyn Fn() -> bool + Sync),
) -> Result<Option<Synced>> {
    let cloned = if checkout.join(".git").exists() {
        git(checkout, &["fetch", "--quiet", "--depth", "1", "origin"]).await?;
        false
    } else {
        clone(checkout, repository).await?;
        true
    };
    let target = if cloned { "HEAD" } else { "FETCH_HEAD" };
    let revision = git(checkout, &["rev-parse", target]).await?;
    let last = index.advisory_revision(SOURCE).await?;
    if last.as_deref() == Some(revision.as_str()) || stop() {
        return Ok(None);
    }

    let changes = match last {
        Some(last) if !cloned && has_commit(checkout, &last).await => {
            Some(changed(checkout, &last, &revision).await?)
        }
        _ => None,
    };
    if !cloned {
        git(checkout, &["reset", "--quiet", "--hard", &revision]).await?;
    }

    let root = checkout.to_path_buf();
    let full = changes.is_none();
    let (upserts, removed) = tokio::task::spawn_blocking(move || match changes {
        Some(changes) => Ok(read_changes(&root, changes)),
        None => read_all(&root.join(REVIEWED)),
    })
    .await
    .context("reading advisories")??;
    if stop() {
        return Ok(None);
    }
    index
        .apply_advisories(AdvisoryImport {
            source: SOURCE,
            revision: &revision,
            full,
            upserts: &upserts,
            removed: &removed,
        })
        .await?;
    info!(
        revision,
        full,
        upserted = upserts.len(),
        removed = removed.len(),
        "advisories: imported"
    );
    Ok(Some(Synced {
        upserted: upserts.len(),
        removed: removed.len(),
        revision,
        full,
    }))
}

/// A shallow clone without file contents, then only the reviewed
/// advisories checked out (their contents fetched in one batch).
async fn clone(checkout: &Path, repository: &str) -> Result<()> {
    if let Some(parent) = checkout.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let target = checkout.to_string_lossy();
    let parent = checkout.parent().unwrap_or_else(|| Path::new("."));
    git(
        parent,
        &[
            "clone",
            "--quiet",
            "--depth",
            "1",
            "--filter=blob:none",
            "--sparse",
            repository,
            &target,
        ],
    )
    .await?;
    git(checkout, &["sparse-checkout", "set", REVIEWED]).await?;
    Ok(())
}

async fn has_commit(checkout: &Path, revision: &str) -> bool {
    git(
        checkout,
        &["cat-file", "-e", &format!("{revision}^{{commit}}")],
    )
    .await
    .is_ok()
}

/// A changed advisory file, by its path in the database.
#[derive(Debug)]
enum Change {
    Written(String),
    Deleted(String),
}

async fn changed(checkout: &Path, from: &str, to: &str) -> Result<Vec<Change>> {
    let out = git(
        checkout,
        &[
            "diff",
            "--name-status",
            "--no-renames",
            from,
            to,
            "--",
            REVIEWED,
        ],
    )
    .await?;
    Ok(out
        .lines()
        .filter_map(|line| {
            let (status, path) = line.split_once('\t')?;
            let path = path.to_owned();
            Some(if status == "D" {
                Change::Deleted(path)
            } else {
                Change::Written(path)
            })
        })
        .filter(|change| match change {
            Change::Written(path) | Change::Deleted(path) => {
                Path::new(path).extension().is_some_and(|ext| ext == "json")
            }
        })
        .collect())
}

fn read_changes(root: &Path, changes: Vec<Change>) -> (Vec<Advisory>, Vec<String>) {
    let mut upserts = Vec::new();
    let mut removed = Vec::new();
    for change in changes {
        match change {
            Change::Deleted(path) => removed.extend(id_of(Path::new(&path))),
            Change::Written(path) => {
                let path = root.join(path);
                match read(&path) {
                    Ok(Some(advisory)) => upserts.push(advisory),
                    // Withdrawn, or about registries vein does not serve.
                    Ok(None) => removed.extend(id_of(&path)),
                    Err(err) => warn!(path = %path.display(), "advisories: skipped: {err:#}"),
                }
            }
        }
    }
    (upserts, removed)
}

fn read_all(dir: &Path) -> Result<(Vec<Advisory>, Vec<String>)> {
    let mut upserts = Vec::new();
    let mut pending: Vec<PathBuf> = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let entries =
            std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?;
        for entry in entries {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "json") {
                match read(&path) {
                    Ok(advisory) => upserts.extend(advisory),
                    Err(err) => warn!(path = %path.display(), "advisories: skipped: {err:#}"),
                }
            }
        }
    }
    Ok((upserts, Vec::new()))
}

fn read(path: &Path) -> Result<Option<Advisory>> {
    let json =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    super::parse(&json)
}

/// `GHSA-xxxx` from `.../GHSA-xxxx/GHSA-xxxx.json`.
fn id_of(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_owned)
}

/// Runs `git args` in `dir`; its trimmed stdout.
async fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .kill_on_drop(true)
        .output()
        .await
        .context("running git (is it installed?)")?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.first().copied().unwrap_or_default(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests;
