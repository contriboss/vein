#![cfg(feature = "crates")]

use vein_adapter::AssetKey;

use super::*;

/// The crate every exports test reads (`tests/fixtures/exports`).
const LIB_RS: &str = include_str!("../../tests/fixtures/exports/my-example/src/lib.rs");
/// Bytes no extractor can read as an archive.
const BROKEN: &[u8] = include_bytes!("../../tests/fixtures/exports/broken.crate");

/// A `.crate`: a gzipped tar holding `{name}-1.0.0/src/lib.rs`.
fn crate_archive(name: &str, lib_rs: &str) -> Vec<u8> {
    use flate2::{Compression, write::GzEncoder};
    use std::io::Write as _;

    let mut archive = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    let mut header = tar::Header::new_gnu();
    header.set_size(lib_rs.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    archive
        .append_data(
            &mut header,
            format!("{name}-1.0.0/src/lib.rs"),
            lib_rs.as_bytes(),
        )
        .unwrap();
    let mut gz = archive.into_inner().unwrap();
    gz.flush().unwrap();
    gz.finish().unwrap()
}

async fn cache(index: &CacheBackend, storage: &FilesystemStorage, name: &str, bytes: &[u8]) {
    let relative = format!("crates/{name}/{name}-1.0.0.crate");
    let path = storage.resolve(&relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
    let key = AssetKey {
        kind: AssetKind::Crate,
        name,
        version: "1.0.0",
        platform: None,
    };
    index
        .insert_or_replace(&key, &relative, "sha", 1)
        .await
        .unwrap();
}

/// What the cache holds is indexed once (same-named items in two modules
/// are two exports); an archive that cannot be read is recorded and left
/// alone; the next run has nothing to do.
#[tokio::test]
async fn cached_versions_are_indexed_once() {
    let index = vein_adapter::test_support::connect_test_backend()
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let storage = FilesystemStorage::new(dir.path().to_path_buf());
    cache(
        &index,
        &storage,
        "my-example",
        &crate_archive("my-example", LIB_RS),
    )
    .await;
    cache(&index, &storage, "broken", BROKEN).await;

    assert_eq!(index_pending(&index, &storage, &|| false).await.unwrap(), 2);

    let paths: Vec<String> = index
        .exports_list(AssetKind::Crate, "my-example", "1.0.0")
        .await
        .unwrap()
        .into_iter()
        .map(|export| export.path)
        .collect();
    assert_eq!(paths.len(), 11, "{paths:?}");
    assert!(
        paths.contains(&"my_example::a::new".to_owned()),
        "{paths:?}"
    );
    assert!(
        paths.contains(&"my_example::b::new".to_owned()),
        "{paths:?}"
    );

    assert_eq!(index_pending(&index, &storage, &|| false).await.unwrap(), 0);
}

/// A cancelled job stops between versions; the rest stay pending.
#[tokio::test]
async fn a_stopped_run_leaves_the_rest_pending() {
    let index = vein_adapter::test_support::connect_test_backend()
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let storage = FilesystemStorage::new(dir.path().to_path_buf());
    cache(
        &index,
        &storage,
        "my-example",
        &crate_archive("my-example", LIB_RS),
    )
    .await;

    assert_eq!(index_pending(&index, &storage, &|| true).await.unwrap(), 0);
    assert_eq!(index_pending(&index, &storage, &|| false).await.unwrap(), 1);
}
