use anyhow::Result;
use rama::http::{Body, HeaderMap, Response, StatusCode, header};

use crate::config::Config;

/// Builds a `no-store` JSON response, letting the caller add request-specific
/// headers (content-length, content-disposition, ...) before the body is set.
fn json_response(
    status: StatusCode,
    body: &str,
    extra: impl FnOnce(&mut HeaderMap) -> Result<()>,
) -> Result<Response<Body>> {
    let mut builder = Response::builder().status(status);
    {
        let headers = builder
            .headers_mut()
            .expect("headers available while building response");
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json; charset=utf-8"),
        );
        headers.insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-store"),
        );
        extra(headers)?;
    }
    builder
        .body(Body::from(body.to_owned()))
        .map_err(Into::into)
}

/// Responds with JSON content
pub fn respond_json(status: StatusCode, body: &str) -> Result<Response<Body>> {
    json_response(status, body, |headers| {
        headers.insert(
            header::CONTENT_LENGTH,
            header::HeaderValue::from_str(&body.len().to_string())?,
        );
        Ok(())
    })
}

/// Responds with JSON content as a downloadable attachment
pub fn respond_json_download(body: &str, filename: &str) -> Result<Response<Body>> {
    json_response(StatusCode::OK, body, |headers| {
        headers.insert(
            header::CONTENT_DISPOSITION,
            header::HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))?,
        );
        Ok(())
    })
}

/// Responds with plain text
pub fn respond_text(status: StatusCode, body: &str) -> Result<Response<Body>> {
    Response::builder()
        .status(status)
        .header(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("text/plain; charset=utf-8"),
        )
        .body(Body::from(body.to_owned()))
        .map_err(Into::into)
}

/// The homepage: what this node serves, and how to point each client at
/// `base`, the URL this client reached it on (`public_base`), so every
/// snippet is copy-paste ready. Each one was checked against a live vein:
/// Cargo cannot replace a source through environment variables, and
/// Bundler's mirror variable needs its trailing slash.
pub fn respond_homepage(config: &Config, base: &str) -> Result<Response<Body>> {
    let storage_root = escape_html(&config.storage.path.display().to_string());
    let body = format!(
        r#"<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>Vein package proxy</title>
    <style>
      :root {{
        color-scheme: light dark;
        --bg: #0f1117;
        --fg: #f4f6ff;
        --accent: #3f8cff;
        --muted: #9aa2b2;
        --code: #1a1e2b;
      }}
      @media (prefers-color-scheme: light) {{
        :root {{
          --bg: #f9fbff;
          --fg: #1b2130;
          --accent: #2563eb;
          --muted: #525f7a;
          --code: #eef2fb;
        }}
      }}
      body {{
        margin: 0;
        background: var(--bg);
        color: var(--fg);
        font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
        line-height: 1.5;
      }}
      main {{
        max-width: 760px;
        margin: 0 auto;
        padding: 3rem 1.5rem;
      }}
      h1 {{ margin-bottom: 0.25rem; }}
      h2 {{ margin-top: 2rem; color: var(--accent); font-size: 1.15rem; }}
      p {{ color: var(--muted); }}
      code, pre {{ font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }}
      pre {{
        background: var(--code);
        padding: 0.9rem 1rem;
        border-radius: 10px;
        overflow-x: auto;
        color: var(--fg);
      }}
    </style>
  </head>
  <body>
    <main>
      <h1>Vein is online</h1>
      <p>
        This node serves RubyGems, crates.io and npm from <code>{base}</code>.
        Point a client at it once: after the first fetch, artifacts come from
        local storage under <code>{storage_root}</code>.
      </p>

      <h2>Bundler</h2>
      <p>Mirror rubygems.org for every project, Gemfiles unchanged:</p>
      <pre>bundle config set --global mirror.https://rubygems.org {base}</pre>
      <p>
        Or per project, in the Gemfile: <code>source "{base}"</code>. For one
        command, the same mirror as an environment variable; its name ends in
        a slash, so shells cannot <code>export</code> it:
      </p>
      <pre>env 'BUNDLE_MIRROR__HTTPS://RUBYGEMS__ORG/={base}' bundle install</pre>

      <h2>RubyGems</h2>
      <pre>gem install rake --clear-sources --source {base}</pre>

      <h2>Cargo</h2>
      <p>In <code>~/.cargo/config.toml</code>, or a project's <code>.cargo/config.toml</code>:</p>
      <pre>[source.crates-io]
replace-with = "vein"

[source.vein]
registry = "sparse+{base}/index/"

# For `cargo search --registry vein`: crates.io's search, through vein.
[registries.vein]
index = "sparse+{base}/index/"</pre>
      <p>
        Cargo cannot replace a source through environment variables
        (<code>CARGO_REGISTRIES_CRATES_IO_INDEX</code> is ignored for crates.io).
        For one command:
      </p>
      <pre>cargo --config 'source.crates-io.replace-with="vein"' \
      --config 'source.vein.registry="sparse+{base}/index/"' build</pre>

      <h2>npm</h2>
      <pre>npm config set registry {base}/</pre>
      <p>Or for one command: <code>NPM_CONFIG_REGISTRY={base}/ npm install</code>.</p>

      <h2>bun</h2>
      <p>In <code>bunfig.toml</code> (a project's, or <code>~/.bunfig.toml</code>):</p>
      <pre>[install]
registry = "{base}/"</pre>
      <p>Or for one command: <code>BUN_CONFIG_REGISTRY={base}/ bun install</code>.</p>

      <h2>CI runners</h2>
      <p>GitHub or Forgejo Actions, before the install steps:</p>
      <pre>env:
  NPM_CONFIG_REGISTRY: {base}/
  BUN_CONFIG_REGISTRY: {base}/
steps:
  - run: bundle config set --global mirror.https://rubygems.org {base}
  - run: |
      mkdir -p ~/.cargo
      cat >> ~/.cargo/config.toml &lt;&lt;'EOF'
      [source.crates-io]
      replace-with = "vein"
      [source.vein]
      registry = "sparse+{base}/index/"
      EOF</pre>
    </main>
  </body>
</html>
"#
    );

    // Rendered from the request's Host: never let a shared cache keep it.
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("text/html; charset=utf-8"),
        )
        .header(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-cache, no-store, must-revalidate"),
        )
        .header(header::PRAGMA, header::HeaderValue::from_static("no-cache"))
        .header(header::EXPIRES, header::HeaderValue::from_static("0"))
        .body(Body::from(body))
        .map_err(Into::into)
}

/// `&`, `<`, `>` and `"` for text placed in the homepage.
fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            other => escaped.push(other),
        }
    }
    escaped
}
