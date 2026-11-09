use vein_adapter::AssetKind;

use super::*;

const CARGO_LOCK: &str = include_str!("../../../tests/fixtures/lockfiles/Cargo.lock");
const GEMFILE_LOCK: &str = include_str!("../../../tests/fixtures/lockfiles/Gemfile.lock");
const PACKAGE_LOCK: &str = include_str!("../../../tests/fixtures/lockfiles/package-lock.json");
const PACKAGE_LOCK_V1: &str =
    include_str!("../../../tests/fixtures/lockfiles/package-lock-v1.json");

fn pinned(text: &str) -> (AssetKind, Vec<String>) {
    let (kind, pinned) = read(text).unwrap();
    let pinned = pinned
        .into_iter()
        .map(|p| format!("{}@{}", p.name, p.version))
        .collect();
    (kind, pinned)
}

/// Registry packages only: not the workspace's own, nor git ones.
#[test]
fn cargo_lock_pins_registry_crates() {
    assert_eq!(
        pinned(CARGO_LOCK),
        (
            AssetKind::Crate,
            vec!["serde@1.0.99".to_owned(), "serde_json@1.0.140".to_owned()]
        )
    );
}

/// `GEM` specs only, platforms dropped; `GIT`, `PATH` and `CHECKSUMS`
/// are not rubygems.org's.
#[test]
fn gemfile_lock_pins_rubygems_gems() {
    assert_eq!(
        pinned(GEMFILE_LOCK),
        (
            AssetKind::Gem,
            vec![
                "nokogiri@1.16.0".to_owned(),
                "racc@1.8.1".to_owned(),
                "rack@3.2.6".to_owned()
            ]
        )
    );
}

/// Nested copies count, links and the root do not.
#[test]
fn package_lock_pins_every_installed_copy() {
    assert_eq!(
        pinned(PACKAGE_LOCK),
        (
            AssetKind::NpmPackage,
            vec![
                "@payloadcms/storage-vercel-blob@3.90.0".to_owned(),
                "filecoin-pin@1.1.0".to_owned(),
                "filecoin-pin@1.1.1".to_owned()
            ]
        )
    );
    assert_eq!(
        pinned(PACKAGE_LOCK_V1),
        (
            AssetKind::NpmPackage,
            vec!["filecoin-pin@1.0.2".to_owned(), "left-pad@1.3.0".to_owned()]
        )
    );
}

#[test]
fn anything_else_is_refused() {
    assert!(read("hello").is_err());
    assert!(read("{ not json").is_err());
}
