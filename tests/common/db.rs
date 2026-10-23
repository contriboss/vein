use sqlx::postgres::PgPool;

#[allow(clippy::missing_const_for_fn)]
pub fn setup_test_db(_pool: &PgPool) {
    // TODO: Run migrations (will be async)
}

#[allow(clippy::missing_const_for_fn)]
pub fn cleanup_test_db(_pool: &PgPool) {
    // TODO: Truncate tables (will be async)
}
