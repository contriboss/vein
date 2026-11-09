use std::path::Path;
use std::process::Command;

use vein_adapter::AssetKind;

use super::*;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/advisories");

/// Runs git in `dir` for the test's own repository.
fn run(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=vein", "-c", "user.email=vein@localhost"])
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// Copies fixture `file` into the database layout, as advisory `id`.
fn publish(origin: &Path, file: &str, id: &str) {
    let dir = origin.join(REVIEWED).join("2026/11").join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(Path::new(FIXTURES).join(file), dir.join(format!("{id}.json"))).unwrap();
}

fn commit(origin: &Path, message: &str) {
    run(origin, &["add", "-A"]);
    run(origin, &["commit", "--quiet", "-m", message]);
}

async fn ids(index: &CacheBackend, kind: AssetKind, name: &str) -> Vec<String> {
    index
        .advisories_for(kind, &[name.to_owned()])
        .await
        .unwrap()
        .into_iter()
        .map(|advisory| advisory.id)
        .collect()
}

/// The first run clones and imports everything; later runs import only
/// what changed: new files, edits, deletions and withdrawals.
#[tokio::test]
async fn imports_everything_then_only_changes() {
    let index = vein_adapter::test_support::connect_test_backend()
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin");
    std::fs::create_dir_all(&origin).unwrap();
    run(&origin, &["init", "--quiet", "--initial-branch", "main"]);
    run(&origin, &["config", "uploadpack.allowFilter", "true"]);
    let unreviewed = origin.join("advisories/unreviewed/2026/11/GHSA-unre-view-ed00");
    std::fs::create_dir_all(&unreviewed).unwrap();
    std::fs::write(unreviewed.join("GHSA-unre-view-ed00.json"), "{}").unwrap();
    publish(&origin, "crate-pageant.json", "GHSA-662x-8wvj-2276");
    publish(&origin, "gem-shopify_app.json", "GHSA-6j52-38f8-qhxr");
    publish(&origin, "pypi-only.json", "GHSA-r4hm-p9f9-fj9v");
    commit(&origin, "first");

    let checkout = dir.path().join("storage/advisory-database");
    let url = format!("file://{}", origin.display());
    let first = sync(&index, &checkout, &url, &|| false).await.unwrap().unwrap();
    assert!(first.full);
    assert_eq!(first.upserted, 2);
    assert_eq!(ids(&index, AssetKind::Crate, "pageant").await, ["GHSA-662x-8wvj-2276"]);
    assert!(
        !checkout.join("advisories/unreviewed").exists(),
        "only reviewed advisories are checked out"
    );

    assert_eq!(sync(&index, &checkout, &url, &|| false).await.unwrap(), None);

    std::fs::remove_dir_all(origin.join(REVIEWED).join("2026/11/GHSA-662x-8wvj-2276")).unwrap();
    publish(&origin, "withdrawn.json", "GHSA-6j52-38f8-qhxr");
    publish(&origin, "npm-two-ranges.json", "GHSA-mc8m-rr6c-r5qr");
    commit(&origin, "second");

    let second = sync(&index, &checkout, &url, &|| false).await.unwrap().unwrap();
    assert!(!second.full);
    assert_eq!((second.upserted, second.removed), (1, 2));
    assert!(ids(&index, AssetKind::Crate, "pageant").await.is_empty());
    assert!(ids(&index, AssetKind::Gem, "shopify_app").await.is_empty());
    assert_eq!(
        ids(&index, AssetKind::NpmPackage, "@payloadcms/storage-vercel-blob").await,
        ["GHSA-mc8m-rr6c-r5qr"]
    );
}

/// A cancelled job imports nothing and records no revision.
#[tokio::test]
async fn a_stopped_run_imports_nothing() {
    let index = vein_adapter::test_support::connect_test_backend()
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin");
    std::fs::create_dir_all(&origin).unwrap();
    run(&origin, &["init", "--quiet", "--initial-branch", "main"]);
    publish(&origin, "crate-pageant.json", "GHSA-662x-8wvj-2276");
    commit(&origin, "first");

    let checkout = dir.path().join("advisory-database");
    let url = format!("file://{}", origin.display());
    assert_eq!(sync(&index, &checkout, &url, &|| true).await.unwrap(), None);
    assert_eq!(index.advisory_revision(SOURCE).await.unwrap(), None);
    assert!(sync(&index, &checkout, &url, &|| false).await.unwrap().is_some());
}
