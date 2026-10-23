use super::*;

use crate::test_support::{install_rustls_provider, raw_response, spawn_sequence_server};

use rama::http::body::util::BodyExt;
use std::sync::Arc;
use tempfile::tempdir;
use vein_adapter::{CacheBackendTrait, FilesystemStorage};

fn make_request(headers: &[(&str, &str)]) -> Request<Body> {
    make_request_at("/lodash", headers)
}

fn make_request_at(path: &str, headers: &[(&str, &str)]) -> Request<Body> {
    let mut builder = Request::builder().method(Method::GET).uri(path);

    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }

    builder.body(Body::empty()).unwrap()
}

#[test]
fn test_is_npm_request_with_npm_command() {
    let req = make_request(&[("npm-command", "install")]);
    assert!(is_npm_request(&req));
}

#[test]
fn test_is_npm_request_with_npm_user_agent() {
    let req = make_request(&[("user-agent", "npm/9.0.0 node/v18.0.0")]);
    assert!(is_npm_request(&req));
}

#[test]
fn test_is_npm_request_with_vnd_accept() {
    let req = make_request(&[("accept", "application/vnd.npm.install-v1+json")]);
    assert!(is_npm_request(&req));
}

#[test]
fn bun_is_recognized_on_metadata_and_tarball_requests() {
    // bun 1.4.3, verbatim: a download carries no npm header at all.
    const BUN_TARBALL: &[(&str, &str)] = &[("user-agent", "Bun/1.4.3"), ("accept", "*/*")];
    // (path, headers, recognized as npm)
    type Case = (&'static str, &'static [(&'static str, &'static str)], bool);
    const CASES: &[Case] = &[
        ("/is-odd/-/is-odd-3.0.1.tgz", BUN_TARBALL, true),
        ("/@babel/core/-/core-7.26.0.tgz", &[("accept", "*/*")], true),
        ("/is-odd", &[("user-agent", "Bun/1.4.3")], true),
        (
            "/is-odd",
            &[("user-agent", "pnpm/9.15.0 npm/? node/v22.0.0 darwin arm64")],
            true,
        ),
        // RubyGems and crates.io paths stay theirs.
        ("/gems/rake-13.4.2.gem", &[("accept", "*/*")], false),
        (
            "/api/v1/crates/itoa/1.0.18/download",
            &[("accept", "*/*")],
            false,
        ),
        ("/-/x.tgz", &[], false),
        ("/@/name/-/x.tgz", &[], false),
        ("/is-odd/-/is-odd-3.0.1.tgz.sha", &[], false),
    ];
    for (path, headers, expected) in CASES {
        assert_eq!(
            is_npm_request(&make_request_at(path, headers)),
            *expected,
            "{path} {headers:?}"
        );
    }
}

#[test]
fn test_is_npm_request_without_npm_headers() {
    let req = make_request(&[("user-agent", "curl/7.0.0")]);
    assert!(!is_npm_request(&req));
}

#[test]
fn test_transform_metadata() {
    let metadata = r#"{
        "name": "lodash",
        "versions": {
            "4.17.21": {
                "dist": {
                    "tarball": "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz",
                    "shasum": "abc123"
                }
            }
        }
    }"#;

    let result = transform_metadata(metadata.as_bytes(), "http://localhost:8346").unwrap();

    let transformed: JsonValue = serde_json::from_slice(&result).unwrap();
    let tarball = transformed["versions"]["4.17.21"]["dist"]["tarball"]
        .as_str()
        .unwrap();
    assert_eq!(tarball, "http://localhost:8346/lodash/-/lodash-4.17.21.tgz");
}

#[test]
fn test_transform_metadata_version_only() {
    let metadata = r#"{
        "name": "lodash",
        "version": "4.17.21",
        "dist": {
            "tarball": "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz"
        }
    }"#;

    let result = transform_metadata(metadata.as_bytes(), "http://localhost:8346").unwrap();

    let transformed: JsonValue = serde_json::from_slice(&result).unwrap();
    let tarball = transformed["dist"]["tarball"].as_str().unwrap();
    assert_eq!(tarball, "http://localhost:8346/lodash/-/lodash-4.17.21.tgz");
}

#[tokio::test]
async fn test_handle_npm_metadata_caches_and_revalidates() {
    install_rustls_provider();

    let client = DefaultHttpWebClient::default();
    let temp_dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(temp_dir.path().join("cache")));
    storage.prepare().await.unwrap();
    let index = Arc::new(
        vein_adapter::test_support::connect_test_backend()
            .await
            .unwrap(),
    );

    let metadata = br#"{
        "name": "lodash",
        "versions": {
            "4.17.21": {
                "dist": {
                    "tarball": "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz"
                }
            }
        }
    }"#;
    let (registry_base, server) = spawn_sequence_server(vec![
        raw_response(
            "200 OK",
            &[
                ("Content-Type", "application/json"),
                ("ETag", "\"lodash-meta-v1\""),
                ("Last-Modified", "Wed, 01 Jan 2025 00:00:00 GMT"),
            ],
            metadata,
        ),
        raw_response("304 Not Modified", &[], &[]),
    ])
    .await;

    let request = Request::builder()
        .method(Method::GET)
        .uri("/lodash")
        .body(Body::empty())
        .unwrap();
    let (response, outcome) = handle_npm_request_from(
        &client,
        request,
        "http://localhost:8346",
        storage.clone(),
        index.clone(),
        &registry_base,
    )
    .await
    .unwrap();
    assert_eq!(outcome, CacheOutcome::Miss);
    let response_body: JsonValue = serde_json::from_slice(&body_bytes(response).await).unwrap();
    let tarball = response_body["versions"]["4.17.21"]["dist"]["tarball"]
        .as_str()
        .unwrap();
    assert_eq!(tarball, "http://localhost:8346/lodash/-/lodash-4.17.21.tgz");

    let request = Request::builder()
        .method(Method::GET)
        .uri("/lodash")
        .body(Body::empty())
        .unwrap();
    let (response, outcome) = handle_npm_request_from(
        &client,
        request,
        "http://localhost:8346",
        storage.clone(),
        index.clone(),
        &registry_base,
    )
    .await
    .unwrap();
    assert_eq!(outcome, CacheOutcome::Revalidated);
    let response_body: JsonValue = serde_json::from_slice(&body_bytes(response).await).unwrap();
    let tarball = response_body["versions"]["4.17.21"]["dist"]["tarball"]
        .as_str()
        .unwrap();
    assert_eq!(tarball, "http://localhost:8346/lodash/-/lodash-4.17.21.tgz");

    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("get /lodash http/1.1"));
    assert!(requests[1].contains("if-none-match: \"lodash-meta-v1\""));
    assert!(requests[1].contains("if-modified-since: wed, 01 jan 2025 00:00:00 gmt"));

    assert!(storage.resolve("npm_index/lodash/metadata.json").exists());
    assert!(
        index
            .catalog_meta_get("npm:metadata:lodash")
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn test_handle_npm_tarball_hits_cache_on_second_request() {
    install_rustls_provider();

    let client = DefaultHttpWebClient::default();
    let temp_dir = tempdir().unwrap();
    let storage = Arc::new(FilesystemStorage::new(temp_dir.path().join("cache")));
    storage.prepare().await.unwrap();
    let index = Arc::new(
        vein_adapter::test_support::connect_test_backend()
            .await
            .unwrap(),
    );

    let tarball = b"fake tgz bytes";
    let (registry_base, server) = spawn_sequence_server(vec![raw_response(
        "200 OK",
        &[("Content-Type", "application/octet-stream")],
        tarball,
    )])
    .await;

    let request = Request::builder()
        .method(Method::GET)
        .uri("/lodash/-/lodash-4.17.21.tgz")
        .body(Body::empty())
        .unwrap();
    let (response, outcome) = handle_npm_request_from(
        &client,
        request,
        "http://localhost:8346",
        storage.clone(),
        index.clone(),
        &registry_base,
    )
    .await
    .unwrap();
    assert_eq!(outcome, CacheOutcome::Miss);
    assert_eq!(body_bytes(response).await, tarball);

    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("get /lodash/-/lodash-4.17.21.tgz http/1.1"));

    let request = Request::builder()
        .method(Method::GET)
        .uri("/lodash/-/lodash-4.17.21.tgz")
        .body(Body::empty())
        .unwrap();
    let (response, outcome) = handle_npm_request_from(
        &client,
        request,
        "http://localhost:8346",
        storage.clone(),
        index.clone(),
        &registry_base,
    )
    .await
    .unwrap();
    assert_eq!(outcome, CacheOutcome::Hit);
    assert_eq!(body_bytes(response).await, tarball);

    assert!(storage.resolve("npm/lodash/lodash-4.17.21.tgz").exists());
}

async fn body_bytes(response: Response<Body>) -> Vec<u8> {
    response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec()
}
