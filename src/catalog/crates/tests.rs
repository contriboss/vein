use jiff::SignedDuration;
use serde_json::json;

use super::*;
use std::time::Duration;

const NEVER: Pacing<'static> = Pacing::new(Duration::ZERO, &|| false);
use crate::test_support::{ok, spawn_route_server};

fn page(crates: &[(&str, &str, Timestamp)], next: Option<&str>) -> Vec<u8> {
    json!({
        "crates": crates.iter().map(|(name, version, updated)| json!({
            "name": name, "max_stable_version": version, "max_version": version,
            "updated_at": updated.to_string(),
        })).collect::<Vec<_>>(),
        "meta": {"next_page": next},
    })
    .to_string()
    .into_bytes()
}

/// The first sync crawls every page and marks the crawl done; the next
/// asks only for crates updated since, and stops at older ones.
#[tokio::test]
async fn crawls_once_then_follows_updates() {
    let index = vein_adapter::test_support::connect_test_backend()
        .await
        .unwrap();
    let old: Timestamp = "2025-01-01T00:00:00Z".parse().unwrap();
    let now = Timestamp::now() + SignedDuration::from_hours(1);
    // MOCKED: crates.io's API; the names, versions and seek token are real.
    let (port, seen) = spawn_route_server(vec![
        (
            "/api/v1/crates?per_page=100&sort=alpha".into(),
            ok(page(
                &[("A-Mazed", "0.1.0", old), ("A2VConverter", "0.1.1", old)],
                Some("?per_page=100&sort=alpha&seek=MTA2MTI1NQ"),
            )),
        ),
        (
            "/api/v1/crates?per_page=100&sort=alpha&seek=MTA2MTI1NQ".into(),
            ok(page(&[("tokio", "1.54.0", old)], None)),
        ),
        (
            "/api/v1/crates?per_page=100&sort=recent-updates".into(),
            ok(page(
                &[("tokio", "1.54.1", now), ("anyhow", "1.0.104", old)],
                Some("?per_page=100&sort=recent-updates&seek=x"),
            )),
        ),
    ])
    .await;
    let api = format!("http://127.0.0.1:{port}/api/v1/crates");
    let client = super::super::build_client();

    assert_eq!(sync(&index, &client, &api, NEVER).await.unwrap(), 3);
    assert_eq!(
        index.catalog_meta_get(META_CRAWL).await.unwrap().as_deref(),
        Some(DONE)
    );

    assert_eq!(sync(&index, &client, &api, NEVER).await.unwrap(), 1);
    let tokio = index
        .names_search(AssetKind::Crate, "tokio", 1)
        .await
        .unwrap();
    assert_eq!(tokio[0].latest_version.as_deref(), Some("1.54.1"));
    assert_eq!(
        index.names_count(AssetKind::Crate).await.unwrap(),
        3,
        "anyhow is older"
    );
    assert!(
        !seen
            .lock()
            .unwrap()
            .iter()
            .any(|request| request.contains("seek=x")),
        "the older update ends the walk"
    );
}

/// A cancelled job's crawl stops after the page it checkpointed, without
/// marking itself done; the next run picks up at the following page.
#[tokio::test]
async fn a_stopped_crawl_resumes_from_its_checkpoint() {
    let index = vein_adapter::test_support::connect_test_backend()
        .await
        .unwrap();
    let old: Timestamp = "2025-01-01T00:00:00Z".parse().unwrap();
    // MOCKED: crates.io's API.
    let (port, seen) = spawn_route_server(vec![
        (
            "/api/v1/crates?per_page=100&sort=alpha".into(),
            ok(page(
                &[("A-Mazed", "0.1.0", old)],
                Some("?per_page=100&sort=alpha&seek=next"),
            )),
        ),
        (
            "/api/v1/crates?per_page=100&sort=alpha&seek=next".into(),
            ok(page(&[("tokio", "1.54.0", old)], None)),
        ),
    ])
    .await;
    let api = format!("http://127.0.0.1:{port}/api/v1/crates");
    let client = super::super::build_client();

    let stopping = Pacing::new(Duration::ZERO, &|| true);
    let err = sync(&index, &client, &api, stopping).await.unwrap_err();
    assert!(err.is::<crate::catalog::Stopped>(), "{err:#}");
    assert_eq!(
        index.catalog_meta_get(META_CRAWL).await.unwrap().as_deref(),
        Some("?per_page=100&sort=alpha&seek=next")
    );
    assert_eq!(index.catalog_meta_get(META_SINCE).await.unwrap(), None);
    assert_eq!(seen.lock().unwrap().len(), 1);

    assert_eq!(sync(&index, &client, &api, NEVER).await.unwrap(), 1);
    assert_eq!(
        index.catalog_meta_get(META_CRAWL).await.unwrap().as_deref(),
        Some(DONE)
    );
    assert_eq!(index.names_count(AssetKind::Crate).await.unwrap(), 2);
}
