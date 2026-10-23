//! The base URL a client reached vein at.
//!
//! Every URL vein hands back (the crates `config.json` download URL, npm
//! tarball URLs, the homepage) must be one the client can reach. Behind
//! mothership or another proxy, the bind address (`server.host:port`, often
//! a loopback) is not: a Cargo client would read the index through the
//! proxy, then try to download every crate from `127.0.0.1`.

use rama::http::{HeaderMap, header};

use crate::config::ServerConfig;

/// `X-Forwarded-Proto` / `X-Forwarded-Host` when a proxy in front sets them,
/// else the `Host` the client sent, else the bind address (an HTTP/1.0
/// client without `Host`).
///
/// A value that is not a plain `host[:port]` is ignored, so a crafted header
/// cannot inject into the HTML or JSON it lands in. Rewritten bodies are
/// never cached (the upstream body is, and the rewrite runs per response),
/// so one client's `Host` never reaches another client.
pub fn public_base(headers: &HeaderMap, server: &ServerConfig) -> String {
    let first = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(str::trim)
    };
    let scheme = match first("x-forwarded-proto") {
        Some(proto) if proto.eq_ignore_ascii_case("https") => "https",
        _ => "http",
    };
    let host = first("x-forwarded-host")
        .filter(|host| is_plain_authority(host))
        .or_else(|| first(header::HOST.as_str()).filter(|host| is_plain_authority(host)));
    match host {
        Some(host) => format!("{scheme}://{host}"),
        None => format!("http://{}:{}", server.host, server.port),
    }
}

/// `example.com`, `10.0.0.5:8080`, `[::1]:8346`: letters, digits, `.`, `-`,
/// `:` and IPv6 brackets only.
fn is_plain_authority(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
}

#[cfg(test)]
mod tests;
