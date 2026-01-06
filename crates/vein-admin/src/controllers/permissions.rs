use loco_rs::prelude::*;

use super::resources;

pub fn routes() -> Routes {
    Routes::new().prefix("permissions").add("/", get(index))
}

#[debug_handler]
async fn index(State(ctx): State<AppContext>) -> Result<Response> {
    // Fetch shared resources so we stay consistent with other controllers once we wire real data.
    let _resources = resources(&ctx)?;

    let html = r#"<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>Vein Admin · Entitlements</title>
    <style>
      :root {
        color-scheme: light dark;
        --bg: #0a0f19;
        --panel: rgba(15, 21, 33, 0.86);
        --border: rgba(99, 113, 140, 0.16);
        --fg: #f4f7ff;
        --muted: rgba(242, 246, 255, 0.72);
        --accent: #38bdf8;
        --accent-soft: rgba(56, 189, 248, 0.22);
      }
      body {
        margin: 0;
        font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
        background:
          radial-gradient(circle at top, rgba(56, 189, 248, 0.14), transparent 55%),
          var(--bg);
        color: var(--fg);
        padding: clamp(2rem, 5vw, 4rem);
      }
      main {
        max-width: 960px;
        margin: auto;
        display: flex;
        flex-direction: column;
        gap: 1.75rem;
      }
      header.top {
        display: flex;
        justify-content: space-between;
        align-items: baseline;
        gap: 1rem;
      }
      header.top h1 {
        margin: 0;
        font-size: clamp(2.1rem, 4vw, 2.75rem);
      }
      header.top a {
        color: var(--muted);
        text-decoration: none;
      }
      header.top a:hover {
        text-decoration: underline;
      }
      nav.links {
        display: flex;
        gap: 1rem;
      }
      nav.links a {
        color: var(--accent);
        font-weight: 600;
        text-decoration: none;
      }
      nav.links a:hover {
        text-decoration: underline;
      }
      .panel {
        padding: clamp(1.75rem, 3vw, 2.5rem);
        border-radius: 22px;
        background: var(--panel);
        border: 1px solid var(--border);
        box-shadow: 0 28px 60px rgba(15, 23, 42, 0.28);
        display: grid;
        gap: 1.5rem;
      }
      .summary {
        display: grid;
        gap: .75rem;
      }
      .summary p {
        margin: 0;
        font-size: 1.05rem;
        line-height: 1.6;
        color: var(--muted);
      }
      .pill {
        display: inline-flex;
        align-items: center;
        gap: .5rem;
        padding: .4rem .9rem;
        border-radius: 999px;
        background: var(--accent-soft);
        color: var(--accent);
        font-weight: 600;
        text-transform: uppercase;
        font-size: .8rem;
        letter-spacing: .12em;
      }
      h2 {
        margin: 0;
        font-size: 1.35rem;
      }
      section.copy {
        display: grid;
        gap: 1rem;
      }
      section.copy p {
        margin: 0;
        color: var(--muted);
        line-height: 1.7;
      }
      .cards {
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
        gap: 1rem;
      }
      .card {
        padding: 1.25rem;
        border-radius: 18px;
        border: 1px solid rgba(148, 163, 184, 0.14);
        background: rgba(10, 16, 28, 0.65);
        display: grid;
        gap: .65rem;
      }
      .card h3 {
        margin: 0;
        font-size: 1.05rem;
      }
      .card ul {
        margin: 0;
        padding-left: 1.25rem;
        display: grid;
        gap: .35rem;
        color: var(--muted);
      }
      .steps {
        counter-reset: step;
        display: grid;
        gap: 1.1rem;
      }
      .steps li {
        list-style: none;
        position: relative;
        padding-left: 2.5rem;
        color: var(--muted);
        line-height: 1.6;
      }
      .steps li::before {
        counter-increment: step;
        content: counter(step);
        position: absolute;
        left: 0;
        top: 0;
        width: 1.9rem;
        height: 1.9rem;
        border-radius: 50%;
        background: var(--accent-soft);
        color: var(--accent);
        display: inline-flex;
        align-items: center;
        justify-content: center;
        font-weight: 600;
      }
      footer.note {
        color: var(--muted);
        font-size: .9rem;
      }
    </style>
  </head>
  <body>
    <main>
      <header class="top">
        <h1>Entitlements &amp; Access Control</h1>
        <a href="/">Back to dashboard</a>
      </header>
      <nav class="links">
        <a href="/">Dashboard</a>
        <a href="/catalog">Catalogue</a>
        <a href="/changelog">Changelog</a>
        <a href="/security">Security</a>
      </nav>

      <div class="panel summary">
        <span class="pill">Design draft</span>
        <p>
          Enterprise instances of Vein will mint short‑lived download grants that describe
          exactly which gems (and versions) a customer is entitled to. Grants are issued
          through an SSH-signed request pipeline so that organisations can reuse existing
          host keys without new credential stores.
        </p>
        <p>
          This page sketches the flow we will implement next. Once the adapter exposes the
          entitlement backend, live metrics will replace the placeholders below.
        </p>
      </div>

      <section class="copy panel">
        <h2>Federated permission model</h2>
        <p>
          Each download token embeds a <code>policy</code> section enumerating gem versions that
          were purchased or approved. The proxy validates the policy before serving cached assets.
          Tokens are signed by an internal signer using the organisation's SSH private key. The
          public key fingerprint doubles as the tenant identifier and allows mirroring nodes to
          trust grants without central coordination.
        </p>
        <p>
          Grant checks will happen at the adapter layer so all endpoints share the same entitlement
          logic. Expired or revoked grants fallback to the upstream mirror, but only for packages
          that the policy allows.
        </p>
      </section>

      <section class="panel">
        <h2>Lifecycle at a glance</h2>
        <ul class="steps">
          <li>Admin registers an SSH public key for the tenant.
            Fingerprint is stored alongside the allowed catalogue scopes.</li>
          <li>Client CLI signs an access request with the tenant host key.
            Payload lists the gems + versions it needs right now.</li>
          <li>Vein issues a JSON Web Token with the request hash,
            allowed scopes, and expiry (15 minutes by default).</li>
          <li>Proxy validates the signature, checks requested gem against the policy,
            and serves the artifact if authorised.</li>
          <li>Background job audits grant usage to spot anomalous access
            and feed billing reports.</li>
        </ul>
      </section>

      <section class="panel cards">
        <article class="card">
          <h3>Token Format</h3>
          <ul>
            <li>Detached JWS signed with ed25519 (via SSH keys)</li>
            <li>Claims: <code>tenant</code>, <code>scopes</code>, <code>versions</code>,
              <code>nonce</code>, <code>exp</code></li>
            <li>Optional <code>mac</code> for offline validation</li>
          </ul>
        </article>
        <article class="card">
          <h3>Enforcement points</h3>
          <ul>
            <li>Request filter before cache lookup</li>
            <li>API downloads and pre-signed URLs</li>
          </ul>
        </article>
        <article class="card">
          <h3>Next workstreams</h3>
          <ul>
            <li>Implement entitlement tables in <code>vein-adapter</code></li>
            <li>Create signer CLI to mint grants locally</li>
            <li>Dashboard widgets for tenant activity + revocations</li>
          </ul>
        </article>
      </section>

      <footer class="note">
        Looking ahead: the dashboard will surface current tenants, latest key rotations,
        and unresolved access violations once the entitlement service is wired up.
      </footer>
    </main>
  </body>
</html>
"#;

    format::html(html)
}
