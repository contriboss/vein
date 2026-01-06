//! Dashboard view helpers.

use chrono::Local;
use serde::Serialize;
use tera::{Context, Tera};

use crate::state::DashboardSnapshot;

#[derive(Debug, Serialize)]
pub struct DashboardData {
    pub generated_at: String,
    pub total_assets: u64,
    pub gems: u64,
    pub unique: u64,
    pub size: String,
    pub catalog_total: u64,
    pub sbom_metric: String,
    pub sbom_detail: String,
    pub ruby_latest: String,
    pub ruby_security: String,
    pub ruby_eol: String,
    pub ruby_updated: String,
    pub storage: String,
    pub database: String,
    pub upstream_detail: String,
    pub last_accessed: String,
    pub endpoint: String,
    pub workers: u64,
}

impl DashboardData {
    pub fn from_snapshot(snapshot: &DashboardSnapshot, show_upstream: bool) -> Self {
        let last_accessed = snapshot.index.last_accessed.as_deref().unwrap_or("never");

        let upstream_value = snapshot
            .upstream
            .as_deref()
            .unwrap_or("offline / cache-only");

        let generated_at = snapshot
            .generated_at
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        let ruby_latest = snapshot
            .ruby_status
            .latest_release
            .as_ref()
            .map(|release| format!("{} ({})", release.version, release.date.format("%Y-%m-%d")))
            .unwrap_or_else(|| "Unknown".to_string());

        let ruby_security = if snapshot.ruby_status.security_maintenance.is_empty() {
            "None".to_string()
        } else {
            snapshot
                .ruby_status
                .security_maintenance
                .iter()
                .map(|branch| {
                    let deadline = branch
                        .expected_eol_date
                        .or(branch.security_maintenance_date)
                        .map(|date| date.format("%Y-%m-%d").to_string())
                        .unwrap_or_else(|| "TBD".to_string());
                    format!("{} (until {})", branch.name, deadline)
                })
                .collect::<Vec<_>>()
                .join(", ")
        };

        let ruby_eol = if snapshot.ruby_status.recent_eol.is_empty() {
            "None".to_string()
        } else {
            snapshot
                .ruby_status
                .recent_eol
                .iter()
                .map(|branch| {
                    let date = branch
                        .eol_date
                        .map(|d| d.format("%Y-%m-%d").to_string())
                        .unwrap_or_else(|| "Unknown".to_string());
                    format!("{} ({})", branch.name, date)
                })
                .collect::<Vec<_>>()
                .join(", ")
        };

        let ruby_updated = snapshot
            .ruby_status
            .fetched_at
            .format("%Y-%m-%d %H:%M UTC")
            .to_string();

        let endpoint = format!("{}:{}", snapshot.server_host, snapshot.server_port);

        let sbom_total = snapshot.sbom.metadata_rows;
        let sbom_with = snapshot.sbom.with_sbom;
        let sbom_missing = sbom_total.saturating_sub(sbom_with);
        let coverage_percent = if sbom_total > 0 {
            (sbom_with as f64 / sbom_total as f64) * 100.0
        } else {
            0.0
        };
        let sbom_metric = if sbom_total > 0 {
            format!("{coverage_percent:.0}%")
        } else {
            "—".to_string()
        };
        let sbom_detail = if sbom_total > 0 {
            format!(
                "{with_sbom} / {total} versions carry SBOMs ({missing} pending)",
                with_sbom = sbom_with,
                total = sbom_total,
                missing = sbom_missing
            )
        } else {
            "SBOMs will appear as soon as gems are cached.".to_string()
        };

        let upstream_detail = if show_upstream {
            upstream_value.to_string()
        } else {
            r#"<span class="muted">hidden</span> &middot; <a href="?upstream=1">show upstream</a>"#
                .to_string()
        };

        Self {
            generated_at,
            total_assets: snapshot.index.total_assets,
            gems: snapshot.index.gem_assets,
            unique: snapshot.index.unique_gems,
            size: format_bytes(snapshot.index.total_size_bytes),
            catalog_total: snapshot.catalog_total,
            sbom_metric,
            sbom_detail,
            ruby_latest,
            ruby_security,
            ruby_eol,
            ruby_updated,
            storage: snapshot.storage_path.display().to_string(),
            database: snapshot.database_path.display().to_string(),
            upstream_detail,
            last_accessed: last_accessed.to_string(),
            endpoint,
            workers: snapshot.worker_count,
        }
    }
}

pub fn index(tera: &Tera, data: DashboardData) -> anyhow::Result<String> {
    let mut context = Context::new();
    context.insert("generated_at", &data.generated_at);
    context.insert("total_assets", &data.total_assets);
    context.insert("gems", &data.gems);
    context.insert("unique", &data.unique);
    context.insert("size", &data.size);
    context.insert("catalog_total", &data.catalog_total);
    context.insert("sbom_metric", &data.sbom_metric);
    context.insert("sbom_detail", &data.sbom_detail);
    context.insert("ruby_latest", &data.ruby_latest);
    context.insert("ruby_security", &data.ruby_security);
    context.insert("ruby_eol", &data.ruby_eol);
    context.insert("ruby_updated", &data.ruby_updated);
    context.insert("storage", &data.storage);
    context.insert("database", &data.database);
    context.insert("upstream_detail", &data.upstream_detail);
    context.insert("last_accessed", &data.last_accessed);
    context.insert("endpoint", &data.endpoint);
    context.insert("workers", &data.workers);

    Ok(tera.render("dashboard/index.html", &context)?)
}

pub fn stats(tera: &Tera, data: DashboardData) -> anyhow::Result<String> {
    stats_fragment(tera, data)
}

pub fn stats_fragment(tera: &Tera, data: DashboardData) -> anyhow::Result<String> {
    let mut context = Context::new();
    context.insert("total_assets", &data.total_assets);
    context.insert("gems", &data.gems);
    context.insert("unique", &data.unique);
    context.insert("size", &data.size);
    context.insert("catalog_total", &data.catalog_total);
    context.insert("sbom_metric", &data.sbom_metric);
    context.insert("sbom_detail", &data.sbom_detail);
    context.insert("ruby_latest", &data.ruby_latest);
    context.insert("ruby_security", &data.ruby_security);
    context.insert("ruby_eol", &data.ruby_eol);
    context.insert("ruby_updated", &data.ruby_updated);

    Ok(tera.render("dashboard/_partials/stats.html", &context)?)
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "0 B".to_string();
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}
