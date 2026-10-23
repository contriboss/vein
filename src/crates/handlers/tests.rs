use super::*;
use crate::config::DelayPolicyConfig;

use crate::test_support::{install_rustls_provider, raw_response, spawn_sequence_server};

use rama::http::body::util::BodyExt;
use std::sync::Arc;
use tempfile::tempdir;
use vein_adapter::{CacheBackendTrait, FilesystemStorage};

#[test]
fn test_serve_index_config() {
    let response = serve_index_config("http://localhost:8346").unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_handle_sparse_index_caches_and_revalidates() {
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

    let body = b"{\"name\":\"serde\"}\n";
    let (upstream_base, server) = spawn_sequence_server(vec![
        raw_response(
            "200 OK",
            &[
                ("Content-Type", "text/plain"),
                ("ETag", "\"serde-v1\""),
                ("Last-Modified", "Wed, 01 Jan 2025 00:00:00 GMT"),
            ],
            body,
        ),
        raw_response("304 Not Modified", &[], &[]),
    ])
    .await;

    let (response, outcome) = handle_sparse_index_from(
        &client,
        "/index/se/rd/serde",
        "http://localhost:8346",
        storage.clone(),
        index.clone(),
        &DelayPolicyConfig::default(),
        &upstream_base,
    )
    .await
    .unwrap();
    assert_eq!(outcome, CacheOutcome::Miss);
    assert_eq!(body_bytes(response).await, body);

    let (response, outcome) = handle_sparse_index_from(
        &client,
        "/index/se/rd/serde",
        "http://localhost:8346",
        storage.clone(),
        index.clone(),
        &DelayPolicyConfig::default(),
        &upstream_base,
    )
    .await
    .unwrap();
    assert_eq!(outcome, CacheOutcome::Revalidated);
    assert_eq!(body_bytes(response).await, body);

    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("get /se/rd/serde http/1.1"));
    assert!(requests[0].contains("accept: text/plain"));
    assert!(requests[1].contains("if-none-match: \"serde-v1\""));
    assert!(requests[1].contains("if-modified-since: wed, 01 jan 2025 00:00:00 gmt"));

    assert!(storage.resolve("crates_index/se/rd/serde").exists());
    assert!(
        index
            .catalog_meta_get("crates:index:serde")
            .await
            .unwrap()
            .is_some()
    );
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
