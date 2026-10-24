//! Dashboard view helpers.

use crate::util::format_bytes;
use jiff::tz::TimeZone;
use serde::Serialize;
use tera::{Context, Tera};

use crate::admin::state::DashboardSnapshot;

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
    /// Ruby's release lifecycle; gem builds only.
    pub ruby: Option<RubyLifecycle>,
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
            #[cfg(feature = "gems")]
            ruby: snapshot.ruby_status.as_deref().map(RubyLifecycle::of),
            #[cfg(not(feature = "gems"))]
            ruby: None,
            storage: snapshot.storage_path.display().to_string(),
            database: snapshot.database.clone(),
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

pub fn index(tera: &Tera, data: &DashboardData) -> anyhow::Result<String> {
    Ok(tera.render("dashboard/index.html", &data.page_context())?)
}

pub fn stats(tera: &Tera, data: &DashboardData) -> anyhow::Result<String> {
    stats_fragment(tera, data)
}

pub fn stats_fragment(tera: &Tera, data: &DashboardData) -> anyhow::Result<String> {
    Ok(tera.render("dashboard/_partials/stats.html", &stats_context(data))?)
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
    context.insert("ruby", &data.ruby);
    context
}

fn formatted_generated_at(snapshot: &DashboardSnapshot) -> String {
    snapshot
        .generated_at
        .to_zoned(TimeZone::system())
        .strftime("%Y-%m-%d %H:%M:%S")
        .to_string()
}

/// Ruby's release lifecycle, as the dashboard shows it.
#[derive(Debug, Serialize)]
pub struct RubyLifecycle {
    pub latest: String,
    pub security: String,
    pub eol: String,
    pub updated: String,
}

#[cfg(feature = "gems")]
impl RubyLifecycle {
    fn of(status: &crate::admin::ruby::RubyStatus) -> Self {
        Self {
            latest: status.latest_release.as_ref().map_or_else(
                || "Unknown".to_string(),
                |release| format!("{} ({})", release.version, release.date),
            ),
            security: Self::security(status),
            eol: Self::eol(status),
            updated: status.fetched_at.strftime("%Y-%m-%d %H:%M UTC").to_string(),
        }
    }

    fn security(status: &crate::admin::ruby::RubyStatus) -> String {
        if status.security_maintenance.is_empty() {
            return "None".to_string();
        }
        status
            .security_maintenance
            .iter()
            .map(|branch| {
                let deadline = branch
                    .expected_eol_date
                    .or(branch.security_maintenance_date)
                    .map_or_else(|| "TBD".to_string(), |date| date.to_string());
                format!("{} (until {})", branch.name, deadline)
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn eol(status: &crate::admin::ruby::RubyStatus) -> String {
        if status.recent_eol.is_empty() {
            return "None".to_string();
        }
        status
            .recent_eol
            .iter()
            .map(|branch| {
                let date = branch
                    .eol_date
                    .map_or_else(|| "Unknown".to_string(), |value| value.to_string());
                format!("{} ({})", branch.name, date)
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
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
