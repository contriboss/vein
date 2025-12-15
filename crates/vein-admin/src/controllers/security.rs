use loco_rs::prelude::*;

use super::resources;

#[derive(Debug)]
struct VulnerableGem {
    name: &'static str,
    version: &'static str,
    cve: &'static str,
    severity: &'static str,
    patched_in: &'static str,
    note: &'static str,
}

fn sample_vulnerabilities() -> Vec<VulnerableGem> {
    vec![
        VulnerableGem {
            name: "nokogiri",
            version: "1.15.4",
            cve: "CVE-2025-12345",
            severity: "critical",
            patched_in: "1.15.6",
            note: "XML entity expansion allows DoS on crafted payloads.",
        },
        VulnerableGem {
            name: "rack",
            version: "2.2.6",
            cve: "CVE-2025-22334",
            severity: "high",
            patched_in: "2.2.8",
            note: "Improper header validation enables request smuggling.",
        },
        VulnerableGem {
            name: "devise",
            version: "4.9.0",
            cve: "CVE-2025-9876",
            severity: "medium",
            patched_in: "4.9.2",
            note: "Token leakage through password reset logs.",
        },
    ]
}

pub fn routes() -> Routes {
    Routes::new().prefix("security").add("/", get(index))
}

#[debug_handler]
async fn index(State(ctx): State<AppContext>) -> Result<Response> {
    let _resources = resources(&ctx)?;
    let rows = sample_vulnerabilities()
        .into_iter()
        .map(|gem| {
            format!(
                r#"<tr class=\"severity-{sev}\">
  <td class=\"gem\">{name}</td>
  <td>{version}</td>
  <td><span>{cve}</span></td>
  <td class=\"severity\">{severity}</td>
  <td>{patched}</td>
  <td>{note}</td>
</tr>"#,
                name = gem.name,
                version = gem.version,
                cve = gem.cve,
                severity = gem.severity.to_uppercase(),
                sev = gem.severity,
                patched = gem.patched_in,
                note = gem.note,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let html = format!(
        r#"<!DOCTYPE html>
<html lang=\"en\">
  <head>
    <meta charset=\"utf-8\" />
    <title>Vein Admin · Security</title>
    <style>
      :root {{
        color-scheme: light dark;
        --bg: #0d1119;
        --panel: rgba(16, 22, 34, 0.85);
        --border: rgba(99, 113, 140, 0.18);
        --fg: #f2f6ff;
        --muted: rgba(242, 246, 255, 0.7);
        --critical: #ef4444;
        --high: #f97316;
        --medium: #facc15;
      }}
      body {{
        margin: 0;
        font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
        background:
          radial-gradient(circle at top, rgba(239, 68, 68, 0.12), transparent 55%),
          var(--bg);
        color: var(--fg);
        padding: clamp(2rem, 5vw, 4rem);
      }}
      main {{
        max-width: 960px;
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
        background: rgba(239, 68, 68, 0.12);
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
      td.severity {{
        font-weight: 700;
        letter-spacing: 0.08em;
      }}
      tr.severity-critical td.severity {{ color: var(--critical); }}
      tr.severity-high td.severity {{ color: var(--high); }}
      tr.severity-medium td.severity {{ color: var(--medium); }}
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
      .banner {{
        padding: 0.75rem 1rem;
        border-radius: 12px;
        background: rgba(239, 68, 68, 0.15);
        border: 1px solid rgba(239, 68, 68, 0.25);
        color: rgba(255, 43, 43, 0.85);
      }}
    </style>
  </head>
  <body>
    <main>
      <header class=\"top\">
        <h1>Security Centre</h1>
        <a href=\"/\">Back to dashboard</a>
      </header>
      <nav class=\"links\">
        <a href=\"/\">Dashboard</a>
        <a href=\"/changelog\">Changelog</a>
        <a href=\"/permissions\">Entitlements</a>
        <a href=\"/catalog\">Catalogue</a>
      </nav>
      <div class=\"banner\">3 packages require attention. Review and promote patched versions.</div>
      <table>
        <thead>
          <tr>
            <th>Gem</th>
            <th>Affected Version</th>
            <th>CVE</th>
            <th>Severity</th>
            <th>Patched in</th>
            <th>Notes</th>
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
        rows = rows,
    );

    format::html(&html)
}
