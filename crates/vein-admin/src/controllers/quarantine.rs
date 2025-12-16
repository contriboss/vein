//! Quarantine management API endpoints.
//!
//! Provides admin UI and API for managing quarantined gem versions:
//! - View quarantine statistics and pending versions
//! - Approve versions for early release
//! - Block malicious versions

use axum::extract::Path;
use loco_rs::prelude::*;
use serde::Deserialize;

use super::resources;

pub fn routes() -> Routes {
    Routes::new()
        .prefix("quarantine")
        .add("/", get(index))
        .add("/api/stats", get(api_stats))
        .add("/api/pending", get(api_pending))
        .add("/:gem/:version/approve", post(approve))
        .add("/:gem/:version/block", post(block))
}

#[derive(Debug, Deserialize)]
pub struct ActionForm {
    reason: Option<String>,
    platform: Option<String>,
}

#[debug_handler]
async fn index(State(ctx): State<AppContext>) -> Result<Response> {
    let resources = resources(&ctx)?;

    if !resources.quarantine_enabled() {
        return format::html(disabled_html());
    }

    let stats = resources
        .quarantine_stats()
        .await
        .map_err(|e| Error::Message(e.to_string()))?;

    let pending = resources
        .quarantine_pending(50, 0)
        .await
        .map_err(|e| Error::Message(e.to_string()))?;

    let rows = pending
        .into_iter()
        .map(|gem| {
            let platform_display = gem.platform.as_deref().unwrap_or("ruby");
            let time_remaining = gem.available_after.signed_duration_since(chrono::Utc::now());
            let hours_remaining = time_remaining.num_hours().max(0);
            let status_class = match gem.status {
                vein_adapter::VersionStatus::Quarantine => "quarantine",
                vein_adapter::VersionStatus::Available => "available",
                vein_adapter::VersionStatus::Yanked => "blocked",
                vein_adapter::VersionStatus::Pinned => "approved",
            };

            format!(
                r#"<tr class="status-{status_class}">
  <td class="gem">{name}</td>
  <td>{version}</td>
  <td>{platform}</td>
  <td class="status">{status}</td>
  <td>{hours}h remaining</td>
  <td class="actions">
    <form method="post" action="/quarantine/{name}/{version}/approve" style="display:inline">
      <input type="hidden" name="platform" value="{platform_raw}">
      <input type="text" name="reason" placeholder="Reason" style="width:120px">
      <button type="submit" class="btn approve">Approve</button>
    </form>
    <form method="post" action="/quarantine/{name}/{version}/block" style="display:inline">
      <input type="hidden" name="platform" value="{platform_raw}">
      <input type="text" name="reason" placeholder="Reason" style="width:120px">
      <button type="submit" class="btn block">Block</button>
    </form>
  </td>
</tr>"#,
                name = gem.name,
                version = gem.version,
                platform = platform_display,
                platform_raw = gem.platform.as_deref().unwrap_or(""),
                status = format!("{:?}", gem.status),
                status_class = status_class,
                hours = hours_remaining,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>Vein Admin - Quarantine</title>
    <style>
      :root {{
        color-scheme: light dark;
        --bg: #0d1119;
        --panel: rgba(16, 22, 34, 0.85);
        --border: rgba(99, 113, 140, 0.18);
        --fg: #f2f6ff;
        --muted: rgba(242, 246, 255, 0.7);
        --quarantine: #f97316;
        --approved: #22c55e;
        --blocked: #ef4444;
      }}
      body {{
        margin: 0;
        font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
        background:
          radial-gradient(circle at top, rgba(249, 115, 22, 0.12), transparent 55%),
          var(--bg);
        color: var(--fg);
        padding: clamp(2rem, 5vw, 4rem);
      }}
      main {{
        max-width: 1200px;
        margin: auto;
        display: flex;
        flex-direction: column;
        gap: 1.5rem;
      }}
      header.top {{
        display: flex;
        justify-content: space-between;
        align-items: baseline;
      }}
      header.top h1 {{
        margin: 0;
        font-size: clamp(2rem, 3.5vw, 2.5rem);
      }}
      header.top a {{
        color: var(--muted);
        text-decoration: none;
      }}
      .stats {{
        display: flex;
        gap: 1rem;
        flex-wrap: wrap;
      }}
      .stat-card {{
        background: var(--panel);
        border: 1px solid var(--border);
        border-radius: 12px;
        padding: 1rem 1.5rem;
        min-width: 150px;
      }}
      .stat-card .value {{
        font-size: 2rem;
        font-weight: 700;
      }}
      .stat-card .label {{
        color: var(--muted);
        font-size: 0.9rem;
      }}
      table {{
        width: 100%;
        border-collapse: collapse;
        background: var(--panel);
        border-radius: 20px;
        overflow: hidden;
        border: 1px solid var(--border);
        box-shadow: 0 30px 55px rgba(15, 23, 42, 0.32);
      }}
      thead {{
        background: rgba(249, 115, 22, 0.12);
      }}
      th, td {{
        padding: 1rem;
        text-align: left;
        font-size: 0.95rem;
        border-bottom: 1px solid rgba(148, 163, 184, 0.08);
      }}
      th {{
        text-transform: uppercase;
        letter-spacing: 0.08em;
        font-size: 0.85rem;
        color: var(--muted);
      }}
      tr:last-child td {{
        border-bottom: none;
      }}
      td.gem {{
        font-weight: 600;
      }}
      td.status {{
        font-weight: 700;
        letter-spacing: 0.06em;
      }}
      tr.status-quarantine td.status {{ color: var(--quarantine); }}
      tr.status-approved td.status {{ color: var(--approved); }}
      tr.status-blocked td.status {{ color: var(--blocked); }}
      .btn {{
        padding: 0.4rem 0.8rem;
        border: none;
        border-radius: 6px;
        cursor: pointer;
        font-size: 0.85rem;
        font-weight: 600;
      }}
      .btn.approve {{
        background: var(--approved);
        color: #000;
      }}
      .btn.block {{
        background: var(--blocked);
        color: #fff;
      }}
      nav.links {{
        display: flex;
        gap: 1rem;
      }}
      nav.links a {{
        color: var(--muted);
        text-decoration: none;
      }}
      nav.links a:hover {{
        text-decoration: underline;
      }}
      input[type="text"] {{
        padding: 0.4rem;
        border: 1px solid var(--border);
        border-radius: 4px;
        background: var(--bg);
        color: var(--fg);
        margin-right: 0.5rem;
      }}
    </style>
  </head>
  <body>
    <main>
      <header class="top">
        <h1>Quarantine Management</h1>
        <a href="/">Back to dashboard</a>
      </header>
      <nav class="links">
        <a href="/">Dashboard</a>
        <a href="/changelog">Changelog</a>
        <a href="/security">Security</a>
        <a href="/catalog">Catalogue</a>
      </nav>
      <section class="stats">
        <div class="stat-card">
          <div class="value">{quarantined}</div>
          <div class="label">Quarantined</div>
        </div>
        <div class="stat-card">
          <div class="value">{available}</div>
          <div class="label">Available</div>
        </div>
        <div class="stat-card">
          <div class="value">{pinned}</div>
          <div class="label">Pinned</div>
        </div>
        <div class="stat-card">
          <div class="value">{yanked}</div>
          <div class="label">Blocked</div>
        </div>
      </section>
      <table>
        <thead>
          <tr>
            <th>Gem</th>
            <th>Version</th>
            <th>Platform</th>
            <th>Status</th>
            <th>Time Remaining</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
        {rows}
        </tbody>
      </table>
    </main>
  </body>
</html>
"#,
        quarantined = stats.total_quarantined,
        available = stats.total_available,
        pinned = stats.total_pinned,
        yanked = stats.total_yanked,
        rows = rows,
    );

    format::html(&html)
}

#[debug_handler]
async fn api_stats(State(ctx): State<AppContext>) -> Result<Response> {
    let resources = resources(&ctx)?;

    if !resources.quarantine_enabled() {
        return format::json(serde_json::json!({
            "enabled": false,
            "error": "Quarantine feature is disabled"
        }));
    }

    let stats = resources
        .quarantine_stats()
        .await
        .map_err(|e| Error::Message(e.to_string()))?;

    format::json(serde_json::json!({
        "enabled": true,
        "quarantined": stats.total_quarantined,
        "available": stats.total_available,
        "pinned": stats.total_pinned,
        "yanked": stats.total_yanked,
        "releasing_today": stats.versions_releasing_today,
        "releasing_this_week": stats.versions_releasing_this_week,
    }))
}

#[derive(Debug, Deserialize)]
pub struct PendingQuery {
    limit: Option<u32>,
    offset: Option<u32>,
}

#[debug_handler]
async fn api_pending(
    State(ctx): State<AppContext>,
    Query(query): Query<PendingQuery>,
) -> Result<Response> {
    let resources = resources(&ctx)?;

    if !resources.quarantine_enabled() {
        return format::json(serde_json::json!({
            "enabled": false,
            "error": "Quarantine feature is disabled"
        }));
    }

    let limit = query.limit.unwrap_or(50).min(100);
    let offset = query.offset.unwrap_or(0);

    let pending = resources
        .quarantine_pending(limit, offset)
        .await
        .map_err(|e| Error::Message(e.to_string()))?;

    let versions: Vec<_> = pending
        .into_iter()
        .map(|v| {
            serde_json::json!({
                "name": v.name,
                "version": v.version,
                "platform": v.platform,
                "status": format!("{:?}", v.status),
                "published_at": v.published_at.to_rfc3339(),
                "available_after": v.available_after.to_rfc3339(),
            })
        })
        .collect();

    format::json(serde_json::json!({
        "enabled": true,
        "versions": versions,
    }))
}

#[debug_handler]
async fn approve(
    State(ctx): State<AppContext>,
    Path((gem, version)): Path<(String, String)>,
    Form(form): Form<ActionForm>,
) -> Result<Response> {
    let resources = resources(&ctx)?;

    if !resources.quarantine_enabled() {
        return format::redirect("/quarantine");
    }

    let platform = form.platform.as_deref().filter(|p| !p.is_empty());
    let reason = form.reason.as_deref().unwrap_or("admin approval");

    resources
        .approve_version(&gem, &version, platform, reason)
        .await
        .map_err(|e| Error::Message(e.to_string()))?;

    tracing::info!(gem = %gem, version = %version, reason = %reason, "Version approved");

    format::redirect("/quarantine")
}

#[debug_handler]
async fn block(
    State(ctx): State<AppContext>,
    Path((gem, version)): Path<(String, String)>,
    Form(form): Form<ActionForm>,
) -> Result<Response> {
    let resources = resources(&ctx)?;

    if !resources.quarantine_enabled() {
        return format::redirect("/quarantine");
    }

    let platform = form.platform.as_deref().filter(|p| !p.is_empty());
    let reason = form.reason.as_deref().unwrap_or("admin blocked");

    resources
        .block_version(&gem, &version, platform, reason)
        .await
        .map_err(|e| Error::Message(e.to_string()))?;

    tracing::warn!(gem = %gem, version = %version, reason = %reason, "Version blocked");

    format::redirect("/quarantine")
}

fn disabled_html() -> &'static str {
    r#"<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>Vein Admin - Quarantine Disabled</title>
    <style>
      :root {
        color-scheme: light dark;
        --bg: #0d1119;
        --fg: #f2f6ff;
        --muted: rgba(242, 246, 255, 0.7);
      }
      body {
        margin: 0;
        font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
        background: var(--bg);
        color: var(--fg);
        padding: clamp(2rem, 5vw, 4rem);
        text-align: center;
      }
      h1 { margin-top: 4rem; }
      p { color: var(--muted); }
      a { color: var(--fg); }
    </style>
  </head>
  <body>
    <h1>Quarantine Disabled</h1>
    <p>The quarantine feature is not enabled in your configuration.</p>
    <p>Enable it by setting <code>delay_policy.enabled = true</code> in vein.toml</p>
    <p><a href="/">Back to dashboard</a></p>
  </body>
</html>
"#
}
