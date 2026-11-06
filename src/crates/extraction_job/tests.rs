use std::io::Write as _;

use flate2::{Compression, write::GzEncoder};

use super::*;

/// A `.crate`: a gzipped tar holding `{name}-{version}/src/lib.rs`.
fn crate_tarball(name: &str, version: &str, lib_rs: &str) -> Vec<u8> {
    let mut archive = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    let mut header = tar::Header::new_gnu();
    header.set_size(lib_rs.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    archive
        .append_data(
            &mut header,
            format!("{name}-{version}/src/lib.rs"),
            lib_rs.as_bytes(),
        )
        .unwrap();
    let mut gz = archive.into_inner().unwrap();
    gz.flush().unwrap();
    gz.finish().unwrap()
}

/// Same-named items in different modules are separate exports.
#[tokio::test]
async fn indexes_every_exported_path_then_removes_the_tarball() {
    let index = vein_adapter::test_support::connect_test_backend()
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let tarball = dir.path().join("demo-0.1.0.crate");
    std::fs::write(
        &tarball,
        crate_tarball(
            "demo",
            "0.1.0",
            "pub mod a { pub fn new() {} }\npub mod b { pub fn new() {} }\npub struct Error;\n",
        ),
    )
    .unwrap();

    let count = extract_into(index.pool(), "demo", "0.1.0", &tarball)
        .await
        .unwrap();

    let paths: Vec<String> = sqlx::query_scalar(
        "SELECT path FROM package_exports WHERE registry = 'crate' AND name = 'demo' ORDER BY path COLLATE \"C\"",
    )
    .fetch_all(index.pool())
    .await
    .unwrap();
    assert_eq!(
        paths,
        [
            "demo::Error",
            "demo::a",
            "demo::a::new",
            "demo::b",
            "demo::b::new"
        ]
    );
    assert_eq!(count, 5);
    assert!(!tarball.exists());
}
