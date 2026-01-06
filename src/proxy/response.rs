use anyhow::Result;
use rama::http::{header, Body, Response, StatusCode};

use crate::config::Config;

/// Responds with JSON content
pub fn respond_json(status: StatusCode, body: &str) -> Result<Response<Body>> {
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
        headers.insert(
            header::CONTENT_LENGTH,
            header::HeaderValue::from_str(&body.len().to_string())?,
        );
    }
    builder
        .body(Body::from(body.to_owned()))
        .map_err(Into::into)
}

/// Responds with JSON content as a downloadable attachment
pub fn respond_json_download(body: &str, filename: &str) -> Result<Response<Body>> {
    let mut builder = Response::builder().status(StatusCode::OK);
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
        headers.insert(
            header::CONTENT_DISPOSITION,
            header::HeaderValue::from_str(&format!("attachment; filename=\"{}\"", filename))?,
        );
    }

    builder
        .body(Body::from(body.to_owned()))
        .map_err(Into::into)
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

/// Responds with the homepage HTML
pub fn respond_homepage(config: &Config) -> Result<Response<Body>> {
    let mut builder = Response::builder().status(StatusCode::OK);
    {
        let headers = builder
            .headers_mut()
            .expect("headers available while building response");
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("text/html; charset=utf-8"),
        );
        headers.insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-cache, no-store, must-revalidate"),
        );
        headers.insert(header::PRAGMA, header::HeaderValue::from_static("no-cache"));
        headers.insert(header::EXPIRES, header::HeaderValue::from_static("0"));
    }

    let body = format!(
        r#"<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>Vein RubyGems Mirror</title>
    <style>
      :root {{
        color-scheme: light dark;
        --bg: #0f1117;
        --fg: #f4f6ff;
        --accent: #3f8cff;
        --muted: #9aa2b2;
      }}
      @media (prefers-color-scheme: light) {{
        :root {{
          --bg: #f9fbff;
          --fg: #1b2130;
          --accent: #2563eb;
          --muted: #525f7a;
        }}
      }}
      body {{
        margin: 0;
        min-height: 100vh;
        display: flex;
        align-items: center;
        justify-content: center;
        background: var(--bg);
        color: var(--fg);
        font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      }}
      main {{
        max-width: 640px;
        padding: 3rem;
        border-radius: 20px;
        background: color-mix(in srgb, var(--bg) 88%, var(--fg) 12%);
        box-shadow: 0 30px 50px rgba(15, 17, 23, 0.20);
      }}
      h1 {{
        font-size: 2.25rem;
        margin: 0 0 1rem;
      }}
      p {{
        margin: 0 0 1.25rem;
        line-height: 1.6;
      }}
      code {{
        display: inline-block;
        padding: 0.2rem 0.45rem;
        border-radius: 8px;
        background: color-mix(in srgb, var(--bg) 70%, var(--fg) 30%);
        color: var(--fg);
        font-size: 0.95rem;
      }}
      ul {{
        margin: 1.5rem 0 0;
        padding: 0;
        list-style: none;
      }}
      li {{
        display: flex;
        align-items: center;
        gap: 0.75rem;
        margin-bottom: 0.5rem;
      }}
      li span {{
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 28px;
        height: 28px;
        border-radius: 50%;
        background: color-mix(in srgb, var(--accent) 35%, transparent);
        color: var(--accent);
        font-weight: 600;
      }}
      a {{
        color: var(--accent);
        text-decoration: none;
        font-weight: 600;
      }}
      a:hover {{
        text-decoration: underline;
      }}
    </style>
  </head>
  <body>
    <main>
      <h1>Vein is online</h1>
      <p>
        This node is proxying RubyGems traffic from
        <code>http://{host}:{port}</code>.
      </p>
      <p>
        Feed it to <strong>ore-light</strong>, Bundler, or your CI runners and cached
        gems will be served from local storage on subsequent requests.
      </p>
      <ul>
        <li><span>1</span>Point clients at this URL as the primary gem source</li>
        <li><span>2</span>Watch the cache fill under <code>./gems</code></li>
      </ul>
    </main>
  </body>
</html>
"#,
        host = config.server.host,
        port = config.server.port
    );

    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("text/html; charset=utf-8"),
        )
        .body(Body::from(body))
        .map_err(Into::into)
}
