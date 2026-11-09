//! The advisories tables: imports from the advisory database, and the
//! lookup `/mcp` makes when an agent reads a package.

use anyhow::{Context, Result};
use jiff_sqlx::ToSqlx;

use super::{PostgresCacheBackend, parse_kind};
use crate::cache::advisory::{Advisory, AdvisoryImport, AffectedPackage};
use crate::cache::types::AssetKind;

type AdvisoryRow = (
    String,
    Vec<String>,
    Option<String>,
    Option<String>,
    Option<jiff_sqlx::Timestamp>,
    jiff_sqlx::Timestamp,
    String,
    String,
    String,
);

impl PostgresCacheBackend {
    /// The commit of `source` last imported, if any.
    pub async fn advisory_revision(&self, source: &str) -> Result<Option<String>> {
        sqlx::query_scalar("SELECT revision FROM advisory_sync WHERE source = $1")
            .bind(source)
            .fetch_optional(&self.pool)
            .await
            .context("reading the advisory revision (postgres)")
    }

    /// Applies `import` and records its revision, in one transaction: a run
    /// cut short leaves the previous revision, to be imported again.
    pub async fn apply_advisories(&self, import: AdvisoryImport<'_>) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .context("starting an advisory import")?;
        if import.full {
            sqlx::query("DELETE FROM advisories")
                .execute(&mut *tx)
                .await
                .context("clearing advisories (postgres)")?;
        }
        sqlx::query("DELETE FROM advisories WHERE id = ANY($1)")
            .bind(import.removed)
            .execute(&mut *tx)
            .await
            .context("removing advisories (postgres)")?;
        for advisory in import.upserts {
            sqlx::query(
                r"
                INSERT INTO advisories (id, aliases, severity, summary, published, modified)
                VALUES ($1, $2, $3, $4, $5, $6)
                ON CONFLICT (id) DO UPDATE SET
                    aliases = EXCLUDED.aliases, severity = EXCLUDED.severity,
                    summary = EXCLUDED.summary, published = EXCLUDED.published,
                    modified = EXCLUDED.modified
                ",
            )
            .bind(&advisory.id)
            .bind(&advisory.aliases)
            .bind(&advisory.severity)
            .bind(&advisory.summary)
            .bind(advisory.published.map(ToSqlx::to_sqlx))
            .bind(advisory.modified.to_sqlx())
            .execute(&mut *tx)
            .await
            .with_context(|| format!("storing advisory {} (postgres)", advisory.id))?;
            sqlx::query("DELETE FROM advisory_packages WHERE advisory_id = $1")
                .bind(&advisory.id)
                .execute(&mut *tx)
                .await
                .context("clearing an advisory's packages (postgres)")?;
            let kinds: Vec<&str> = advisory.packages.iter().map(|p| p.kind.as_str()).collect();
            let names: Vec<&str> = advisory.packages.iter().map(|p| p.name.as_str()).collect();
            let affected = advisory
                .packages
                .iter()
                .map(|p| serde_json::to_string(&p.affected))
                .collect::<Result<Vec<_>, _>>()
                .context("encoding affected versions")?;
            sqlx::query(
                r"
                INSERT INTO advisory_packages (advisory_id, kind, name, affected)
                SELECT $1, kind, name, affected::JSONB
                FROM UNNEST($2::TEXT[], $3::TEXT[], $4::TEXT[]) AS t(kind, name, affected)
                ",
            )
            .bind(&advisory.id)
            .bind(&kinds)
            .bind(&names)
            .bind(&affected)
            .execute(&mut *tx)
            .await
            .with_context(|| format!("storing advisory {}'s packages (postgres)", advisory.id))?;
        }
        sqlx::query(
            r"
            INSERT INTO advisory_sync (source, revision) VALUES ($1, $2)
            ON CONFLICT (source) DO UPDATE SET revision = EXCLUDED.revision, synced_at = NOW()
            ",
        )
        .bind(import.source)
        .bind(import.revision)
        .execute(&mut *tx)
        .await
        .context("recording the advisory revision (postgres)")?;
        tx.commit().await.context("committing an advisory import")
    }

    /// Every advisory about packages `names` of registry `kind`, newest
    /// first; each lists only those packages.
    pub async fn advisories_for(&self, kind: AssetKind, names: &[String]) -> Result<Vec<Advisory>> {
        let rows = sqlx::query_as::<_, AdvisoryRow>(
            r"
            SELECT a.id, a.aliases, a.severity, a.summary, a.published, a.modified,
                   p.kind, p.name, p.affected::TEXT
            FROM advisory_packages p JOIN advisories a ON a.id = p.advisory_id
            WHERE p.kind = $1 AND p.name = ANY($2)
            ORDER BY a.published DESC NULLS LAST, a.id
            ",
        )
        .bind(kind.as_str())
        .bind(names)
        .fetch_all(&self.pool)
        .await
        .context("reading advisories (postgres)")?;

        let mut advisories: Vec<Advisory> = Vec::new();
        for (id, aliases, severity, summary, published, modified, kind, name, affected) in rows {
            let package = AffectedPackage {
                kind: parse_kind(&kind)?,
                name,
                affected: serde_json::from_str(&affected)
                    .with_context(|| format!("reading advisory {id}'s affected versions"))?,
            };
            match advisories.last_mut() {
                Some(last) if last.id == id => last.packages.push(package),
                _ => advisories.push(Advisory {
                    id,
                    aliases,
                    severity,
                    summary,
                    published: published.map(jiff_sqlx::Timestamp::to_jiff),
                    modified: modified.to_jiff(),
                    packages: vec![package],
                }),
            }
        }
        Ok(advisories)
    }
}
