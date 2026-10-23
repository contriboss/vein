// Integration tests run against a real database.
// Run: VEIN_TEST_DATABASE_URL=postgres://... cargo test --test '*'

mod common;

#[tokio::test]
async fn test_db_connection() {
    let pool = common::test_pool()
        .await
        .expect("failed to connect to test db");
    common::db::setup_test_db(&pool);
    common::db::cleanup_test_db(&pool);
}
