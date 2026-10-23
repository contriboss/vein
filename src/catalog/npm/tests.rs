use serde_json::json;

use super::*;
use crate::test_support::{ok, spawn_route_server};

/// The walk pages through `_all_docs` (the replica has no `skip`, so each
/// page repeats the cursor row), stores every package but `CouchDB`'s
/// design documents, then follows the change feed from the walk's starting
/// sequence, adding and removing.
#[tokio::test]
async fn walks_all_docs_then_follows_changes() {
    let index = vein_adapter::test_support::connect_test_backend()
        .await
        .unwrap();
    // A full first page: real ids, then generated ones up to 10 000 rows.
    let first_page: Vec<String> = ["-", "_design/app", "left-pad"]
        .into_iter()
        .map(str::to_owned)
        .chain((0..9_997).map(|i| format!("pkg-{i:05}")))
        .collect();
    let rows = |ids: &[String]| json!({"rows": ids.iter().map(|id| json!({"id": id})).collect::<Vec<_>>()});
    // MOCKED: npm's replica; the real ids and sequences come from it.
    let (port, seen) = spawn_route_server(vec![
        (
            "/".into(),
            ok(json!({"db_name": "registry", "update_seq": 139_259_226}).to_string()),
        ),
        (
            "/_all_docs?limit=10000".into(),
            ok(rows(&first_page).to_string()),
        ),
        (
            // Inclusive: the cursor row comes back first.
            "/_all_docs?limit=10000&startkey=%22pkg%2D09996%22".into(),
            ok(rows(&["pkg-09996".into(), "zz-last".into()]).to_string()),
        ),
        (
            "/_changes?since=139259226&limit=10000".into(),
            ok(json!({
                "results": [
                    {"seq": 139_259_202, "id": "create-strata"},
                    {"seq": 139_259_207, "id": "left-pad", "deleted": true},
                ],
                "last_seq": 139_259_207,
            })
            .to_string()),
        ),
        (
            "/_changes?since=139259207&limit=10000".into(),
            ok(json!({"results": [], "last_seq": 139_259_207}).to_string()),
        ),
    ])
    .await;
    let base = format!("http://127.0.0.1:{port}");
    let client = super::super::build_client();

    // 9 999 packages, then `zz-last`; then one added and one removed.
    assert_eq!(
        sync(&index, &client, &base, Duration::ZERO).await.unwrap(),
        10_002
    );
    assert!(
        !seen
            .lock()
            .unwrap()
            .iter()
            .any(|request| request.contains("skip=")),
        "the replica answers `skip` with 400"
    );
    let found = |query: &'static str| {
        let index = &index;
        async move {
            index
                .names_search(AssetKind::NpmPackage, query, 2)
                .await
                .unwrap()
                .into_iter()
                .map(|entry| entry.name)
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(found("zz-last").await, ["zz-last"], "the second page");
    assert_eq!(
        found("create-strata").await,
        ["create-strata"],
        "from the change feed"
    );
    let nothing: Vec<String> = Vec::new();
    assert_eq!(found("left-pad").await, nothing, "left-pad was unpublished");
    assert_eq!(
        found("_design").await,
        nothing,
        "design documents are not packages"
    );
    assert_eq!(
        index.names_count(AssetKind::NpmPackage).await.unwrap(),
        10_000
    );
    assert_eq!(
        index.catalog_meta_get(META_SEQ).await.unwrap().as_deref(),
        Some("139259207")
    );
}
