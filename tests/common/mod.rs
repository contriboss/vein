// Integration test utilities and shared setup.
// Requires: VEIN_TEST_DATABASE_URL=postgres://... cargo test
// Local container: vein-test-pg:55432

pub mod db;

use anyhow::Result;
use sqlx::postgres::PgPool;

pub async fn test_pool() -> Result<PgPool> {
    let url = std::env::var("VEIN_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:55432/vein_test".into());
    Ok(PgPool::connect(&url).await?)
}
