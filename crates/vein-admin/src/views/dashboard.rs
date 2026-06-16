//! Dashboard view helpers.

use chrono::Local;
use serde::Serialize;
use tera::{Context, Tera};
use vein::util::format_bytes;

use crate::state::DashboardSnapshot;

#[derive(Debug, Serialize)]
pub struct DashboardData {
    pub generated_at: String,
    pub total_assets: u64,
    pub rubygems_assets: u64,
    pub crate_assets: u64,
    pub npm_assets: u64,
    pub unique_packages: u64,
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
        let sbom = SbomSummary::from_snapshot(snapshot);

        Self {
            generated_at: formatted_generated_at(snapshot),
            total_assets: snapshot.index.total_assets,
            rubygems_assets: snapshot.index.rubygems_assets,
            crate_assets: snapshot.index.crate_assets,
            npm_assets: snapshot.index.npm_assets,
            unique_packages: snapshot.index.unique_packages,
            size: format_bytes(snapshot.index.total_size_bytes),
            catalog_total: snapshot.catalog_total,
            sbom_metric: sbom.metric,
            sbom_detail: sbom.detail,
            ruby_latest: ruby_latest_label(snapshot),
            ruby_security: ruby_security_label(snapshot),
            ruby_eol: ruby_eol_label(snapshot),
            ruby_updated: ruby_updated_label(snapshot),
            storage: snapshot.storage_path.display().to_string(),
            database: snapshot.database_path.display().to_string(),
            upstream_detail: upstream_detail(snapshot, show_upstream),
            last_accessed: snapshot
                .index
                .last_accessed
                .as_deref()
                .unwrap_or("never")
                .to_string(),
            endpoint: format!("{}:{}", snapshot.server_host, snapshot.server_port),
            workers: snapshot.worker_count,
        }
    }

    fn page_context(&self) -> Context {
        let mut context = stats_context(self);
        context.insert("current_page", "dashboard");
        context.insert("generated_at", &self.generated_at);
        context.insert("storage", &self.storage);
        context.insert("database", &self.database);
        context.insert("upstream_detail", &self.upstream_detail);
        context.insert("last_accessed", &self.last_accessed);
        context.insert("endpoint", &self.endpoint);
        context.insert("workers", &self.workers);
        context
    }
}

pub fn index(tera: &Tera, data: DashboardData) -> anyhow::Result<String> {
    Ok(tera.render("dashboard/index.html", &data.page_context())?)
}

pub fn stats(tera: &Tera, data: DashboardData) -> anyhow::Result<String> {
    stats_fragment(tera, data)
}

pub fn stats_fragment(tera: &Tera, data: DashboardData) -> anyhow::Result<String> {
    Ok(tera.render("dashboard/_partials/stats.html", &stats_context(&data))?)
}

#[derive(Debug)]
struct SbomSummary {
    metric: String,
    detail: String,
}

impl SbomSummary {
    fn from_snapshot(snapshot: &DashboardSnapshot) -> Self {
        let total = snapshot.sbom.metadata_rows;
        let with_sbom = snapshot.sbom.with_sbom;
        let missing = total.saturating_sub(with_sbom);

        if total == 0 {
            return Self {
                metric: "—".to_string(),
                detail: "SBOMs will appear as soon as RubyGems artifacts are cached.".to_string(),
            };
        }

        let coverage_percent = (with_sbom as f64 / total as f64) * 100.0;
        Self {
            metric: format!("{coverage_percent:.0}%"),
            detail: format!("{with_sbom} / {total} versions carry SBOMs ({missing} pending)"),
        }
    }
}

fn stats_context(data: &DashboardData) -> Context {
    let mut context = Context::new();
    context.insert("total_assets", &data.total_assets);
    context.insert("rubygems_assets", &data.rubygems_assets);
    context.insert("crate_assets", &data.crate_assets);
    context.insert("npm_assets", &data.npm_assets);
    context.insert("unique_packages", &data.unique_packages);
    context.insert("size", &data.size);
    context.insert("catalog_total", &data.catalog_total);
    context.insert("sbom_metric", &data.sbom_metric);
    context.insert("sbom_detail", &data.sbom_detail);
    context.insert("ruby_latest", &data.ruby_latest);
    context.insert("ruby_security", &data.ruby_security);
    context.insert("ruby_eol", &data.ruby_eol);
    context.insert("ruby_updated", &data.ruby_updated);
    context
}

fn formatted_generated_at(snapshot: &DashboardSnapshot) -> String {
    snapshot
        .generated_at
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

fn ruby_latest_label(snapshot: &DashboardSnapshot) -> String {
    snapshot
        .ruby_status
        .latest_release
        .as_ref()
        .map(|release| format!("{} ({})", release.version, release.date.format("%Y-%m-%d")))
        .unwrap_or_else(|| "Unknown".to_string())
}

fn ruby_security_label(snapshot: &DashboardSnapshot) -> String {
    if snapshot.ruby_status.security_maintenance.is_empty() {
        return "None".to_string();
    }

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
}

fn ruby_eol_label(snapshot: &DashboardSnapshot) -> String {
    if snapshot.ruby_status.recent_eol.is_empty() {
        return "None".to_string();
    }

    snapshot
        .ruby_status
        .recent_eol
        .iter()
        .map(|branch| {
            let date = branch
                .eol_date
                .map(|value| value.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            format!("{} ({})", branch.name, date)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn ruby_updated_label(snapshot: &DashboardSnapshot) -> String {
    snapshot
        .ruby_status
        .fetched_at
        .format("%Y-%m-%d %H:%M UTC")
        .to_string()
}

fn upstream_detail(snapshot: &DashboardSnapshot, show_upstream: bool) -> String {
    let upstream = snapshot
        .upstream
        .as_deref()
        .unwrap_or("offline / cache-only");

    if show_upstream {
        upstream.to_string()
    } else {
        r#"<span class="muted">hidden</span> &middot; <a href="?upstream=1">show upstream</a>"#
            .to_string()
    }
}
