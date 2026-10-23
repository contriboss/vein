use rama::http::body::util::BodyExt;
use vein_adapter::AssetKey;

use super::*;
use crate::test_support::{install_rustls_provider, ok, spawn_route_server};

#[test]
fn search_requests_in_each_dialect() {
    let query = |kind, text: &str, limit| {
        Some(Query {
            kind,
            text: text.into(),
            limit,
        })
    };
    let cases = [
        // What `cargo search serde --limit 5 --registry vein` sends.
        (
            "/api/v1/crates?q=serde&per_page=5",
            query(AssetKind::Crate, "serde", 5),
        ),
        // What `npm search jiff cron` sends.
        (
            "/-/v1/search?text=jiff%20cron&size=20&from=0&quality=0.65",
            query(AssetKind::NpmPackage, "jiff cron", 20),
        ),
        (
            "/api/v1/search.json?query=rack",
            query(AssetKind::Gem, "rack", 30),
        ),
        (
            "/api/v1/crates?q=tokio&per_page=1000",
            query(AssetKind::Crate, "tokio", 100),
        ),
        // A crate download, and a crates listing without a query.
        ("/api/v1/crates/serde/1.0.228/download", None),
        ("/api/v1/crates?page=2", None),
    ];
    for (target, expected) in cases {
        assert_eq!(
            Query::parse(&Uri::from_static(target)),
            expected,
            "{target}"
        );
    }
}

#[test]
fn names_come_out_of_every_format() {
    let entry = |name: &str| CatalogName {
        name: name.into(),
        latest_version: Some("1.0.0".into()),
    };
    let entries = [entry("serde"), entry("serde_json")];
    for kind in [AssetKind::Gem, AssetKind::Crate, AssetKind::NpmPackage] {
        let body = in_registry_format(kind, &entries);
        assert_eq!(names(kind, &body), ["serde", "serde_json"], "{kind:?}");
    }
    assert_eq!(
        in_registry_format(AssetKind::Crate, &entries)["crates"][0]["max_version"],
        "1.0.0"
    );
}

async fn answer(
    registries: &Registries,
    index: &CacheBackend,
    target: &'static str,
) -> (String, Value) {
    install_rustls_provider();
    let query = Query::parse(&Uri::from_static(target)).unwrap();
    let response = respond(
        &DefaultHttpWebClient::default(),
        index,
        registries,
        &query,
        target,
    )
    .await
    .unwrap();
    let source = response.headers()[SOURCE_HEADER]
        .to_str()
        .unwrap()
        .to_owned();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (source, serde_json::from_slice(&body).unwrap())
}

/// The registry's answer passes through; without it, the catalog answers,
/// and without a catalog, the names cached here.
#[tokio::test]
async fn the_registry_answers_else_the_catalog_else_the_cache() {
    let index = vein_adapter::test_support::connect_test_backend()
        .await
        .unwrap();
    // MOCKED: crates.io's search, abridged from its real answer for `jiff`.
    let crates_io = serde_json::json!({
        "crates": [{"name": "jiff", "max_version": "0.2.38", "description": "A date-time library"}],
        "meta": {"total": 1},
    });
    let (port, _) = spawn_route_server(vec![(
        "/api/v1/crates?q=jiff&per_page=10".into(),
        ok(crates_io.to_string()),
    )])
    .await;
    let online = Registries {
        crates: format!("http://127.0.0.1:{port}"),
        ..Registries::default()
    };
    assert_eq!(
        answer(&online, &index, "/api/v1/crates?q=jiff&per_page=10").await,
        ("registry".into(), crates_io)
    );

    // Nothing listens on port 1: the registry is out of reach.
    let offline = Registries {
        crates: "http://127.0.0.1:1".into(),
        npm: "http://127.0.0.1:1".into(),
        ..Registries::default()
    };
    let (source, cached) = answer(&offline, &index, "/-/v1/search?text=left").await;
    assert_eq!(
        (source.as_str(), names(AssetKind::NpmPackage, &cached)),
        ("cached", Vec::new())
    );
    let key = AssetKey {
        kind: AssetKind::NpmPackage,
        name: "left-pad",
        version: "1.3.0",
        platform: None,
    };
    index
        .insert_or_replace(&key, "npm/left-pad-1.3.0.tgz", "ff", 1)
        .await
        .unwrap();
    let (source, cached) = answer(&offline, &index, "/-/v1/search?text=left").await;
    assert_eq!(
        (source.as_str(), names(AssetKind::NpmPackage, &cached)),
        ("cached", vec!["left-pad".to_owned()])
    );

    index
        .names_upsert(
            AssetKind::Crate,
            &[CatalogName {
                name: "jiff".into(),
                latest_version: Some("0.2.38".into()),
            }],
        )
        .await
        .unwrap();
    let (source, catalog) = answer(&offline, &index, "/api/v1/crates?q=jiff&per_page=10").await;
    assert_eq!(source, "catalog");
    assert_eq!(catalog["crates"][0]["max_version"], "0.2.38");
}

/// npm's search leaves deprecated packages out: asked for `left-pad`, its
/// own answer lacks it. The catalog knows the name, so it comes first; an
/// answer that has the exact name passes through byte for byte.
#[tokio::test]
async fn an_exact_name_comes_first_even_when_the_registry_leaves_it_out() {
    let index = vein_adapter::test_support::connect_test_backend()
        .await
        .unwrap();
    index
        .names_upsert(
            AssetKind::NpmPackage,
            &[CatalogName {
                name: "left-pad".into(),
                latest_version: Some("1.3.0".into()),
            }],
        )
        .await
        .unwrap();
    // MOCKED: npm's search; the names are its real answer for `left-pad`.
    let without = serde_json::json!({
        "objects": [{"package": {"name": "pad-left"}}, {"package": {"name": "@stdlib/string-left-pad"}}],
        "total": 2,
    });
    let with = r#"{"objects":[{"package":{"name":"left-pad"}}],"total":1}"#;
    let (port, _) = spawn_route_server(vec![
        ("/-/v1/search?text=left-pad".into(), ok(without.to_string())),
        // The route server matches case-insensitively; `size` tells the two apart.
        ("/-/v1/search?text=left-pad&size=1".into(), ok(with)),
    ])
    .await;
    let online = Registries {
        npm: format!("http://127.0.0.1:{port}"),
        ..Registries::default()
    };

    let (source, found) = answer(&online, &index, "/-/v1/search?text=left-pad").await;
    assert_eq!(source, "registry");
    assert_eq!(
        names(AssetKind::NpmPackage, &found),
        ["left-pad", "pad-left", "@stdlib/string-left-pad"]
    );
    assert_eq!(found["objects"][0]["package"]["version"], "1.3.0");

    let (_, found) = answer(&online, &index, "/-/v1/search?text=Left-Pad&size=1").await;
    assert_eq!(
        found,
        serde_json::from_str::<Value>(with).unwrap(),
        "already there: untouched"
    );
}
