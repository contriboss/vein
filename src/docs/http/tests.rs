use std::sync::Arc;

use rama::http::body::util::BodyExt;
use vein_adapter::FilesystemStorage;

use super::*;
use crate::config::Config;
#[cfg(feature = "gems")]
use crate::docs::test_support::state_machines_gem;
#[cfg(feature = "gems")]
use crate::test_support::build_test_gem;
#[cfg(feature = "crates")]
use crate::test_support::tar_gz;
#[cfg(any(feature = "gems", feature = "crates"))]
use crate::test_support::{ok, spawn_route_server};

#[test]
fn docs_paths_name_registry_package_version_and_what() {
    let parsed = |path| {
        parse(path).map(|request| {
            request.ok().map(|request| match request {
                Request::Readme(p) => (p.kind, p.name, p.version, None),
                Request::Api(p, at) => (p.kind, p.name, p.version, Some(at)),
            })
        })
    };
    // A registry built out is a docs path, refused.
    let readme = |kind, name: &str, version: &str| {
        Some(crate::registry::built(kind).then(|| (kind, name.into(), version.into(), None)))
    };
    let api = |kind, name: &str, version: &str, at: Option<&str>| {
        Some(
            crate::registry::built(kind)
                .then(|| (kind, name.into(), version.into(), Some(at.map(Into::into)))),
        )
    };
    assert_eq!(
        parsed("/docs/gems/state_machines/0.202.0"),
        readme(AssetKind::Gem, "state_machines", "0.202.0")
    );
    assert_eq!(
        parsed("/docs/npm/@types/node/26.6.5"),
        readme(AssetKind::NpmPackage, "@types/node", "26.6.5")
    );
    assert_eq!(
        parsed("/docs/crates/state-machines/0.32.1/api"),
        api(AssetKind::Crate, "state-machines", "0.32.1", None)
    );
    assert_eq!(
        parsed("/docs/crates/state-machines/0.32.1/api/runtime::Runner"),
        api(
            AssetKind::Crate,
            "state-machines",
            "0.32.1",
            Some("runtime::Runner")
        )
    );
    assert_eq!(
        parsed("/docs/gems/state_machines/0.202.0/api/StateMachines::Machine%23action_hook%3F"),
        api(
            AssetKind::Gem,
            "state_machines",
            "0.202.0",
            Some("StateMachines::Machine#action_hook?")
        ),
        "percent-decoded"
    );
    assert_eq!(
        parsed("/docs/crates/a/b/1.0.0"),
        Some(None),
        "a crate name has no `/`"
    );
    assert_eq!(
        parsed("/docs/pypi/requests/2.32.3"),
        None,
        "not a registry vein serves"
    );
    assert_eq!(parsed("/docs/gems/rack"), None, "no version");
}

async fn get(docs: &Docs, path: &str) -> (StatusCode, String, String) {
    let response = respond(docs, path).await.unwrap().unwrap();
    let status = response.status();
    let content_type = response.headers()[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .to_owned();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        content_type,
        String::from_utf8(body.to_vec()).unwrap(),
    )
}

/// `Docs` with an empty cache, the proxy at `port`; the storage directory
/// lives as long as the guard.
async fn docs_behind(port: u16, mut config: Config) -> (Docs, tempfile::TempDir) {
    config.server.host = "127.0.0.1".into();
    config.server.port = port;
    let storage = tempfile::tempdir().unwrap();
    let docs = Docs::new(
        Arc::new(config),
        Arc::new(
            vein_adapter::test_support::connect_test_backend()
                .await
                .unwrap(),
        ),
        Arc::new(FilesystemStorage::new(storage.path().to_path_buf())),
    );
    (docs, storage)
}

/// Not cached: the archive comes through the proxy, the README out of it.
#[cfg(feature = "crates")]
#[tokio::test]
async fn a_readme_comes_out_of_the_archive_through_the_proxy() {
    // The real opening of state-machines 0.32.1's README; its Cargo.toml
    // names it.
    let readme =
        "# state-machines\n\n> **A learning-focused Rust port of Ruby's state_machines gem**\n";
    let krate = tar_gz(&[
        (
            "state-machines-0.32.1/Cargo.toml",
            b"[package]\nname = \"state-machines\"\nversion = \"0.32.1\"\nreadme = \"README.md\"\n",
        ),
        ("state-machines-0.32.1/README.md", readme.as_bytes()),
    ]);
    // MOCKED: vein's proxy, serving one .crate.
    let (port, _) = spawn_route_server(vec![(
        "/api/v1/crates/state-machines/0.32.1/download".into(),
        ok(krate),
    )])
    .await;
    let (docs, _storage) = docs_behind(port, Config::default()).await;

    let (status, content_type, body) = get(&docs, "/docs/crates/state-machines/0.32.1").await;
    assert_eq!(
        (status, content_type.as_str()),
        (StatusCode::OK, "text/markdown; charset=utf-8")
    );
    assert_eq!(body, readme.trim_end());

    let (status, _, body) = get(&docs, "/docs/crates/state-machines/9.9.9").await;
    assert_eq!(
        (status, body.as_str()),
        (
            StatusCode::NOT_FOUND,
            "state-machines 9.9.9 is not published\n"
        )
    );
    let (status, _, _) = get(&docs, "/docs/crates/..%2f/1.0.0").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// The API from docs.rs, fetched once: the second read is from vein's
/// storage, which the fake docs.rs, answering only once, proves.
#[cfg(feature = "crates")]
#[tokio::test]
async fn crate_api_docs_come_from_docs_rs_once() {
    let zst = ruzstd::encoding::compress_to_vec(
        super::super::test_support::state_machines()
            .to_string()
            .as_bytes(),
        ruzstd::encoding::CompressionLevel::Fastest,
    );
    // MOCKED: docs.rs, with one version's rustdoc JSON.
    let (port, seen) =
        spawn_route_server(vec![("/crate/state-machines/0.32.1/json".into(), ok(zst))]).await;
    let mut config = Config::default();
    config.docs.docs_rs = format!("http://127.0.0.1:{port}");
    let (docs, _storage) = docs_behind(port, config).await;

    let (status, content_type, outline) =
        get(&docs, "/docs/crates/state-machines/0.32.1/api").await;
    assert_eq!(
        (status, content_type.as_str()),
        (StatusCode::OK, "text/markdown; charset=utf-8")
    );
    assert!(outline.starts_with("# state_machines 0.32.1"), "{outline}");
    let (status, _, runner) = get(
        &docs,
        "/docs/crates/state-machines/0.32.1/api/runtime::Runner",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        runner.contains("fn dispatch<E>(&mut self, event: E)"),
        "{runner}"
    );
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "cached after the first fetch"
    );

    let (status, _, body) = get(
        &docs,
        "/docs/crates/state-machines/0.32.1/api/runtime::Nope",
    )
    .await;
    assert_eq!(
        (status, body.as_str()),
        (
            StatusCode::NOT_FOUND,
            "state-machines 0.32.1 has no public `runtime::Nope`\n"
        )
    );
    let (status, _, body) = get(&docs, "/docs/crates/state-machines/0.1.0/api").await;
    assert_eq!(
        (status, body.as_str()),
        (
            StatusCode::NOT_FOUND,
            "no API docs for state-machines 0.1.0\n"
        )
    );
}

/// A gem's API out of the Ruby in its `.gem`, through the proxy; a method's
/// page at its percent-encoded `#`.
#[cfg(feature = "gems")]
#[tokio::test]
async fn gem_api_docs_come_from_its_ruby_source() {
    let bare = build_test_gem(
        "--- !ruby/object:Gem::Specification\nname: state_machines\n",
        &[("README.md", b"# State Machines\n")],
    );
    // MOCKED: vein's proxy, serving two .gem files.
    let (port, _) = spawn_route_server(vec![
        (
            "/gems/state_machines-0.202.0.gem".into(),
            ok(state_machines_gem()),
        ),
        (
            "/gems/state_machines-0.0.1.gem".into(),
            ok(std::fs::read(bare.path()).unwrap()),
        ),
    ])
    .await;
    let (docs, _storage) = docs_behind(port, Config::default()).await;

    let response = respond(&docs, "/docs/gems/state_machines/0.202.0/api")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        response.headers()["x-vein-docs-source"],
        "the gem's Ruby source"
    );
    let (status, _, outline) = get(&docs, "/docs/gems/state_machines/0.202.0/api").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        outline.starts_with("# state_machines 0.202.0: Ruby API"),
        "{outline}"
    );
    let (status, _, method) = get(
        &docs,
        "/docs/gems/state_machines/0.202.0/api/StateMachines::Machine%23action_hook%3F",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        method.starts_with("# `Machine#action_hook?(self_only = false)`"),
        "{method}"
    );

    let (status, _, body) = get(
        &docs,
        "/docs/gems/state_machines/0.202.0/api/StateMachines::Nope",
    )
    .await;
    assert_eq!(
        (status, body.as_str()),
        (
            StatusCode::NOT_FOUND,
            "state_machines 0.202.0 has no public `StateMachines::Nope`\n"
        )
    );
    let (status, _, body) = get(&docs, "/docs/gems/state_machines/0.0.1/api").await;
    assert_eq!(
        (status, body.as_str()),
        (
            StatusCode::NOT_FOUND,
            "no API docs for state_machines 0.0.1\n"
        )
    );
}

/// Docs of a registry built out: 404, saying why.
#[tokio::test]
async fn docs_of_a_registry_built_out_are_not_found() {
    let (docs, _storage) = docs_behind(1, Config::default()).await;
    for (kind, path) in [
        (AssetKind::Gem, "/docs/gems/state_machines/0.202.0"),
        (AssetKind::Crate, "/docs/crates/state-machines/0.32.1/api"),
        (AssetKind::NpmPackage, "/docs/npm/left-pad/1.3.0"),
    ] {
        if crate::registry::built(kind) {
            continue;
        }
        let (status, _, body) = get(&docs, path).await;
        assert_eq!(
            (status, body),
            (StatusCode::NOT_FOUND, format!("{}\n", NotBuilt(kind))),
            "{path}"
        );
    }
}
