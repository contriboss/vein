//! Database operations for crate exports.

use anyhow::Result;
use sqlx::PgPool;

use super::exports::Export;

/// Insert extracted crate exports into database.
/// Uses upsert to handle re-runs gracefully.
pub async fn insert_crate_exports(
    pool: &PgPool,
    name: &str,
    version: &str,
    exports: Vec<Export>,
) -> Result<()> {
    if exports.is_empty() {
        return Ok(());
    }

    let mut tx = pool.begin().await?;

    for export in exports {
        sqlx::query(
            r"
            INSERT INTO package_exports
              (registry, name, version, symbol, kind, path, docs, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
            ON CONFLICT (registry, name, version, symbol)
            DO UPDATE SET
              kind = EXCLUDED.kind,
              path = EXCLUDED.path,
              docs = EXCLUDED.docs,
              created_at = NOW()
            ",
        )
        .bind("crate")
        .bind(name)
        .bind(version)
        .bind(&export.name)
        .bind(export.kind.to_string())
        .bind(&export.path)
        .bind(&export.docs)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}
