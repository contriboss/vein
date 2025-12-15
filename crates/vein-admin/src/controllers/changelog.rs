use chrono::NaiveDate;
use loco_rs::prelude::*;

use super::resources;

pub fn routes() -> Routes {
    Routes::new().prefix("changelog").add("/", get(index))
}

#[derive(Debug)]
struct ChangeLogEntry {
    date: NaiveDate,
    title: &'static str,
    category: &'static str,
    details: &'static str,
    highlight: bool,
}

fn sample_entries() -> Vec<ChangeLogEntry> {
    vec![
        ChangeLogEntry {
            date: NaiveDate::from_ymd_opt(2025, 10, 28).unwrap(),
            title: "Ruby 3.4.7 Available",
            category: "Ruby Release",
            details: "New security and performance update. \
                      Ruby 3.3.x enters maintenance mode on Dec 01.",
            highlight: true,
        },
        ChangeLogEntry {
            date: NaiveDate::from_ymd_opt(2025, 10, 25).unwrap(),
            title: "New Gem: turbo-latest",
            category: "New Gem",
            details: "turbo-latest 1.0.0 published with native Apple Silicon builds.",
            highlight: false,
        },
        ChangeLogEntry {
            date: NaiveDate::from_ymd_opt(2025, 10, 19).unwrap(),
            title: "EOL Reminder: Rails 6.1",
            category: "EOL Notice",
            details: "Rails 6.1 will exit support on Nov 15. Upgrade paths to 7.2+.",
            highlight: true,
        },
        ChangeLogEntry {
            date: NaiveDate::from_ymd_opt(2025, 10, 12).unwrap(),
            title: "Maintenance Pause: elasticsearch-ruby",
            category: "Maintenance",
            details: "Upstream maintainers announced limited maintenance while API stabilises.",
            highlight: false,
        },
    ]
}

#[debug_handler]
async fn index(State(ctx): State<AppContext>) -> Result<Response> {
    // Touch shared resources to keep consistent with other pages (may carry stats later).
    let _resources = resources(&ctx)?;
    let entries = sample_entries();

    let list = entries
        .iter()
        .map(|entry| {
            format!(
                r#"<article class=\"entry {highlight}\">
  <header>
    <span class=\"date\">{date}</span>
    <span class=\"category\">{category}</span>
    <h2>{title}</h2>
  </header>
  <p>{details}</p>
</article>"#,
                date = entry.date.format("%b %d, %Y"),
                category = entry.category,
                title = entry.title,
                details = entry.details,
                highlight = if entry.highlight { "accent" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let html = format!(
        r#"<!DOCTYPE html>
<html lang=\"en\">
  <head>
    <meta charset=\"utf-8\" />
    <title>Vein Admin · Changelog</title>
    <style>
      :root {{
        color-scheme: light dark;
        --bg: #0b1018;
        --panel: rgba(18, 24, 36, 0.82);
        --border: rgba(148, 163, 184, 0.14);
        --accent: #8b5cf6;
        --accent-soft: rgba(139, 92, 246, 0.18);
        --fg: #f5f8ff;
        --muted: rgba(241, 245, 255, 0.7);
      }}
      body {{
        margin: 0;
        font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
        background:
          linear-gradient(180deg, rgba(139, 92, 246, 0.06) 0%, transparent 40%),
          var(--bg);
        color: var(--fg);
        padding: clamp(2rem, 5vw, 4rem);
      }}
      main {{
        max-width: 860px;
        margin: auto;
        display: flex;
        flex-direction: column;
        gap: 1.75rem;
      }}
      header.top {{
        display: flex;
        justify-content: space-between;
        align-items: baseline;
      }}
      header.top h1 {{
        margin: 0;
        font-size: clamp(2rem, 4vw, 2.6rem);
      }}
      header.top a {{
        color: var(--accent);
        text-decoration: none;
      }}
      .entry {{
        padding: 1.5rem;
        border-radius: 18px;
        background: var(--panel);
        border: 1px solid var(--border);
        box-shadow: 0 26px 40px rgba(15, 23, 42, 0.25);
      }}
      .entry.accent {{
        border-color: var(--accent);
        box-shadow: 0 0 0 1px rgba(139, 92, 246, 0.4);
      }}
      .entry header {{
        display: flex;
        flex-wrap: wrap;
        gap: 0.75rem;
        align-items: center;
      }}
      .entry h2 {{
        flex: 1 1 auto;
        margin: 0;
        font-size: 1.25rem;
      }}
      .entry .date {{
        font-weight: 600;
        letter-spacing: 0.08em;
        text-transform: uppercase;
        color: var(--muted);
      }}
      .entry .category {{
        font-size: 0.85rem;
        padding: 0.25rem 0.6rem;
        border-radius: 999px;
        background: rgba(139, 92, 246, 0.18);
        color: var(--accent);
        text-transform: uppercase;
        letter-spacing: 0.05em;
      }}
      nav.links {{
        display: flex;
        gap: 1rem;
        margin-bottom: .5rem;
      }}
      nav.links a {{
        color: var(--accent);
        text-decoration: none;
      }}
      nav.links a:hover {{
        text-decoration: underline;
      }}
    </style>
  </head>
  <body>
    <main>
      <header class=\"top\">
        <h1>Vein Changelog</h1>
        <a href=\"/\">Back to dashboard</a>
      </header>
      <nav class=\"links\">
        <a href=\"/\">Dashboard</a>
        <a href=\"/security\">Security</a>
        <a href=\"/permissions\">Entitlements</a>
        <a href=\"/catalog\">Catalogue</a>
      </nav>
      {entries}
    </main>
  </body>
</html>
"#,
        entries = list,
    );

    format::html(&html)
}
