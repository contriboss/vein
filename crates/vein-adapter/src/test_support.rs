//! Cache backends for tests: a private schema on the Postgres named by
//! [`DATABASE_URL_ENV`], one per call, so tests never see each other's rows.

use anyhow::{Result, anyhow};

use crate::CacheBackend;

/// Postgres URL the test suite may create and drop schemas in.
pub const DATABASE_URL_ENV: &str = "VEIN_TEST_DATABASE_URL";

/// A cache backend that no other test can see.
///
/// # Errors
///
/// When [`DATABASE_URL_ENV`] is unset, the database is unreachable, or
/// migrations fail.
pub async fn connect_test_backend() -> Result<CacheBackend> {
    let url = std::env::var(DATABASE_URL_ENV).map_err(|_| {
        anyhow!("set {DATABASE_URL_ENV} to a Postgres URL the tests may create schemas in")
    })?;
    CacheBackend::connect_isolated(&url).await
}
