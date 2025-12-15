use chrono::prelude::*;
use loco_rs::prelude::*;
use serde::Deserialize;

use super::resources;
use crate::state::DashboardSnapshot;

#[derive(Debug, Deserialize, Default)]
struct DashboardQuery {
    #[serde(default)]
    upstream: Option<String>,
}

pub fn routes() -> Routes {
    Routes::new().add("/", get(index))
}

#[debug_handler]
async fn index(
    State(ctx): State<AppContext>,
    Query(query): Query<DashboardQuery>,
) -> Result<Response> {
    let resources = resources(&ctx)?;
    let snapshot = resources
        .snapshot()
        .await
        .map_err(|err| Error::Message(err.to_string()))?;

    let show_upstream = query.upstream.is_some();
    let html = render_dashboard(&snapshot, show_upstream);
    format::html(&html)
}

fn render_dashboard(snapshot: &DashboardSnapshot, show_upstream: bool) -> String {
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

    let ruby_latest_raw = snapshot
        .ruby_status
        .latest_release
        .as_ref()
        .map(|release| format!("{} ({})", release.version, release.date.format("%Y-%m-%d")))
        .unwrap_or_else(|| "Unknown".to_string());

    let ruby_security_raw = if snapshot.ruby_status.security_maintenance.is_empty() {
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

    let ruby_eol_raw = if snapshot.ruby_status.recent_eol.is_empty() {
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

    let ruby_updated_raw = snapshot
        .ruby_status
        .fetched_at
        .format("%Y-%m-%d %H:%M UTC")
        .to_string();

    let ruby_latest = escape_html(&ruby_latest_raw);
    let ruby_security = escape_html(&ruby_security_raw);
    let ruby_eol = escape_html(&ruby_eol_raw);
    let ruby_updated = escape_html(&ruby_updated_raw);
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
    let sbom_metric = escape_html(&sbom_metric);
    let sbom_detail = escape_html(&sbom_detail);

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>Vein Dashboard</title>
    <style>
      :root {{
        color-scheme: light dark;
        --bg: #0b1018;
        --fg: #f4f7ff;
        --muted: #96a1b7;
        --panel: rgba(17, 23, 34, 0.75);
        --border: rgba(148, 163, 184, 0.2);
        --accent: #4f8cff;
        --accent-soft: rgba(79, 140, 255, 0.2);
      }}
      @media (prefers-color-scheme: light) {{
        :root {{
          --bg: #f5f7fc;
          --fg: #101522;
          --muted: #4c566f;
          --panel: rgba(255, 255, 255, 0.85);
          --border: rgba(15, 23, 42, 0.08);
          --accent: #1d4ed8;
          --accent-soft: rgba(29, 78, 216, 0.08);
        }}
      }}
      * {{
        box-sizing: border-box;
      }}
      body {{
        margin: 0;
        min-height: 100vh;
        font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
        background:
          radial-gradient(circle at top, rgba(79, 140, 255, 0.06), transparent 55%),
          var(--bg);
        color: var(--fg);
        display: flex;
        padding: 4rem clamp(2rem, 5vw, 5rem);
      }}
      main {{
        margin: auto;
        width: min(960px, 100%);
        display: flex;
        flex-direction: column;
        gap: 2.5rem;
      }}
      header {{
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 1.5rem;
        padding: clamp(1.75rem, 3vw, 2.5rem);
        border-radius: 24px;
        background: var(--panel);
        border: 1px solid var(--border);
        box-shadow: 0 40px 80px rgba(15, 23, 42, 0.2);
        backdrop-filter: blur(20px);
      }}
      header h1 {{
        margin: 0;
        font-size: clamp(1.75rem, 4vw, 2.3rem);
        letter-spacing: -0.03em;
      }}
      header p {{
        margin: .35rem 0 0;
        color: var(--muted);
        font-size: clamp(0.95rem, 1.8vw, 1rem);
      }}
      nav.links {{
        display: flex;
        gap: 1rem;
        margin: -1.5rem 0 1rem;
        padding: 0 clamp(1.75rem, 3vw, 2.5rem);
      }}
      nav.links a {{
        color: var(--accent);
        text-decoration: none;
        font-weight: 600;
      }}
      nav.links a:hover {{
        text-decoration: underline;
      }}
      .pill {{
        display: inline-flex;
        align-items: center;
        gap: .5rem;
        padding: .5rem 1rem;
        border-radius: 999px;
        border: 1px solid var(--border);
        background: rgba(255,255,255,0.02);
        color: var(--muted);
        font-size: .9rem;
      }}
      .grid {{
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
        gap: 1.5rem;
      }}
      .card {{
        padding: 1.6rem;
        border-radius: 20px;
        background: var(--panel);
        border: 1px solid var(--border);
        box-shadow: 0 24px 48px rgba(15, 23, 42, 0.18);
        display: flex;
        flex-direction: column;
        gap: .85rem;
      }}
      .card h2 {{
        margin: 0;
        font-size: .95rem;
        text-transform: uppercase;
        letter-spacing: .14em;
        color: var(--muted);
      }}
      .card .metric {{
        font-size: clamp(1.8rem, 3.4vw, 2.4rem);
        font-weight: 600;
      }}
      .detail {{
        margin-top: auto;
        font-size: .9rem;
        color: var(--muted);
        line-height: 1.5;
      }}
      .muted {{
        color: var(--muted);
      }}
      .panel {{
        padding: 1.75rem;
        border-radius: 20px;
        background: var(--panel);
        border: 1px solid var(--border);
        box-shadow: 0 20px 45px rgba(15, 23, 42, 0.16);
        display: grid;
        gap: 1rem;
      }}
      .feature-box {{
        display: grid;
        gap: 1.1rem;
        padding: clamp(1.75rem, 3vw, 2.3rem);
        border-radius: 22px;
        background: var(--panel);
        border: 1px solid var(--border);
        box-shadow: 0 26px 45px rgba(15, 23, 42, 0.18);
      }}
      .feature-box h2 {{
        margin: 0;
        font-size: 1.1rem;
        text-transform: uppercase;
        letter-spacing: .12em;
        color: var(--muted);
      }}
      .feature-box ul {{
        list-style: none;
        margin: 0;
        padding: 0;
        display: grid;
        gap: .75rem;
      }}
      .feature-box li {{
        display: flex;
        align-items: center;
        gap: .8rem;
        font-size: 1rem;
        color: var(--fg);
      }}
      .feature-box li span.icon {{
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 1.5rem;
        height: 1.5rem;
        border-radius: 50%;
        background: var(--accent-soft);
        color: var(--accent);
        font-weight: 700;
        font-size: .9rem;
      }}
      .panel dl {{
        margin: 0;
        display: grid;
        grid-template-columns: 180px 1fr;
        gap: .75rem 1.5rem;
        font-size: .95rem;
      }}
      .panel dt {{
        font-weight: 600;
        color: var(--muted);
        text-transform: uppercase;
        letter-spacing: .1em;
      }}
      .panel dd {{
        margin: 0;
        font-family: 'JetBrains Mono', 'SFMono-Regular', Menlo, monospace;
        font-size: .95rem;
        color: var(--fg);
      }}
      a {{
        color: var(--accent);
        text-decoration: none;
      }}
      a:hover {{
        text-decoration: underline;
      }}
    </style>
  </head>
  <body>
    <main>
      <header>
        <div>
          <h1>Vein Admin Console</h1>
          <p>Live cache insight and node configuration snapshot</p>
        </div>
        <div class="pill">
          <span>Generated {generated_at}</span>
        </div>
      </header>
      <nav class="links">
        <a href="/catalog">Catalogue</a>
        <a href="/changelog">Changelog</a>
        <a href="/permissions">Entitlements</a>
        <a href="/security">Security</a>
      </nav>

      <section class="grid">
        <article class="card">
          <h2>Total Assets</h2>
          <div class="metric">{total_assets}</div>
          <p class="detail">Includes gems and gemspecs cached locally.</p>
        </article>
        <article class="card">
          <h2>Cached Gems</h2>
          <div class="metric">{gems}</div>
          <p class="detail">Unique gem files stored on disk.</p>
        </article>
        <article class="card">
          <h2>Unique Packages</h2>
          <div class="metric">{unique}</div>
          <p class="detail">Distinct gem names currently cached.</p>
        </article>
        <article class="card">
          <h2>Storage Footprint</h2>
          <div class="metric">{size}</div>
          <p class="detail">Approximate disk usage of cached assets.</p>
        </article>
        <article class="card">
          <h2>Catalogue Size</h2>
          <div class="metric">{catalog_total}</div>
          <p class="detail">Upstream gem names synced. <a href="/catalog">Browse catalogue</a>.</p>
        </article>
        <article class="card">
          <h2>SBOM Coverage</h2>
          <div class="metric">{sbom_metric}</div>
          <p class="detail">{sbom_detail}</p>
        </article>
        <article class="card">
          <h2>Ruby Lifecycle</h2>
          <p><strong>Latest:</strong> {ruby_latest}</p>
          <p><strong>Security maintenance:</strong> {ruby_security}</p>
          <p><strong>Recent EOL:</strong> {ruby_eol}</p>
          <p class="detail">Fetched {ruby_updated}</p>
        </article>
        <article class="card">
          <h2>Access Control</h2>
          <p><strong>Status:</strong> Entitlements design draft</p>
          <p>SSH-signed tokens will gate premium gems and version ranges.</p>
          <p class="detail"><a href="/permissions">Review entitlement plan</a></p>
        </article>
      </section>

      <section class="feature-box">
        <h2>Features enabled</h2>
        <ul>
          <li><span class="icon">&#10003;</span>Rubygems protocol</li>
          <li><span class="icon">&#10003;</span>Upstream protocol bridge</li>
          <li><span class="icon">&#10003;</span>SBOM export stream</li>
          <li><span class="icon">&#10003;</span>Gem auto-updater</li>
          <li><span class="icon">&#10003;</span>Diff-aware gem delivery</li>
          <li><span class="icon">&#10003;</span>SSH entitlement signing</li>
          <li><span class="icon">&#10003;</span>Incremental catalogue sync</li>
          <li><span class="icon">&#10003;</span>Ruby lifecycle insights</li>
        </ul>
      </section>

      <section class="panel">
        <dl>
      <dt>Storage Path</dt>
      <dd>{storage}</dd>

      <dt>Index Database</dt>
      <dd>{database}</dd>

      <dt>Upstream</dt>
      <dd>{upstream_detail}</dd>

      <dt>Last Access</dt>
      <dd>{last_accessed}</dd>

      <dt>Proxy Endpoint</dt>
      <dd>{endpoint}</dd>

      <dt>Workers</dt>
      <dd>{workers}</dd>
    </dl>
      </section>
    </main>
  </body>
</html>
"#,
        total_assets = snapshot.index.total_assets,
        gems = snapshot.index.gem_assets,
        unique = snapshot.index.unique_gems,
        size = format_bytes(snapshot.index.total_size_bytes),
        storage = escape_html(&snapshot.storage_path.display().to_string()),
        database = escape_html(&snapshot.database_path.display().to_string()),
        upstream_detail = if show_upstream {
            escape_html(upstream_value)
        } else {
            "<span class=\"muted\">hidden</span> &middot; <a href=\"?upstream=1\">show upstream</a>"
                .to_string()
        },
        catalog_total = snapshot.catalog_total,
        sbom_metric = sbom_metric,
        sbom_detail = sbom_detail,
        ruby_latest = ruby_latest,
        ruby_security = ruby_security,
        ruby_eol = ruby_eol,
        ruby_updated = ruby_updated,
        last_accessed = escape_html(last_accessed),
        endpoint = escape_html(&endpoint),
        workers = snapshot.worker_count,
    )
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

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}
