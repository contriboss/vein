#![cfg(feature = "crates")]

//! MCP resource: `vein://crates/{name}@{version}/exports`

use anyhow::Result;
use rama::telemetry::tracing::debug;
use serde_json::{Value, json};
use sqlx::PgPool;

/// Query crate exports by name and version.
#[allow(dead_code)]
pub async fn get_exports(pool: &PgPool, name: &str, version: &str) -> Result<Value> {
    debug!("fetching exports for {name}@{version}");

    let rows = sqlx::query_as::<_, (String, String, String, Option<String>)>(
        "SELECT symbol, kind, path, docs FROM package_exports
         WHERE registry = 'crate' AND name = $1 AND version = $2
         ORDER BY symbol ASC",
    )
    .bind(name)
    .bind(version)
    .fetch_all(pool)
    .await?;

    let exports: Vec<Value> = rows
        .into_iter()
        .map(|(symbol, kind, path, docs)| {
            json!({
                "symbol": symbol,
                "kind": kind,
                "path": path,
                "docs": docs
            })
        })
        .collect();

    Ok(json!({
        "name": name,
        "version": version,
        "exports": exports,
        "count": exports.len()
    }))
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_get_exports_empty() {
        // This test would require a real database connection
        // For now, just verify compilation
    }
}
