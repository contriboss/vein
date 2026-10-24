//! Which registry a request is for, decided before any registry's code runs.
//!
//! A registry this build left out answers 404 rather than reaching another
//! registry's upstream: an npm package name has no business at rubygems.org.

use rama::http::{Body, Request, header};
use vein_adapter::AssetKind;

use crate::search;

/// The registry `req` speaks to, when vein can tell; `None` for vein's own
/// paths (`/`, `/up`, `/docs/...`) and anything else.
#[must_use]
pub fn registry_of(req: &Request<Body>) -> Option<AssetKind> {
    if let Some(query) = search::Query::parse(req.uri()) {
        return Some(query.kind);
    }
    if is_npm_request(req) {
        return Some(AssetKind::NpmPackage);
    }
    let path = req.uri().path_or_root();
    let path = path.as_ref();
    if path.starts_with("/index/") || path.starts_with("/api/v1/crates/") {
        return Some(AssetKind::Crate);
    }
    let gem = ["/gems/", "/quick/", "/info/", "/api/v1/"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
        || matches!(
            path,
            "/versions"
                | "/names"
                | "/specs.4.8.gz"
                | "/latest_specs.4.8.gz"
                | "/prerelease_specs.4.8.gz"
                | "/.well-known/vein/sbom"
        );
    gem.then_some(AssetKind::Gem)
}

/// Check if a request is from an npm client: a tarball path, or npm headers.
///
/// bun sends neither `npm-command` nor an npm `Accept` type when it downloads
/// a tarball (`User-Agent: Bun/1.4.3`, `Accept: */*`), so its downloads are
/// recognized by path and its metadata requests by user agent. pnpm and
/// yarn name npm in their user agents (`pnpm/9 npm/? node/...`).
pub fn is_npm_request(req: &Request<Body>) -> bool {
    if is_tarball_path(&req.uri().path_or_root()) {
        return true;
    }

    // Check for npm-command header (most reliable)
    if req.headers().contains_key("npm-command") {
        return true;
    }

    // Check User-Agent for npm-compatible clients
    if let Some(ua) = req.headers().get(header::USER_AGENT)
        && let Ok(ua_str) = ua.to_str()
        && (ua_str.starts_with("npm/") || ua_str.contains(" npm/") || ua_str.starts_with("Bun/"))
    {
        return true;
    }

    // Check Accept header for npm-specific types
    if let Some(accept) = req.headers().get(header::ACCEPT)
        && let Ok(accept_str) = accept.to_str()
        && accept_str.contains("application/vnd.npm")
    {
        return true;
    }

    false
}

/// npm's tarball URLs, `/<name>/-/<file>.tgz` and `/@scope/<name>/-/<file>.tgz`.
/// No gem or crate path has a `-` segment followed by a `.tgz` file.
fn is_tarball_path(path: &str) -> bool {
    let Some(rest) = path.strip_prefix('/') else {
        return false;
    };
    let tgz = |file: &str| {
        std::path::Path::new(file)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("tgz"))
    };
    let segments: Vec<&str> = rest.split('/').collect();
    match segments.as_slice() {
        [name, "-", file] => !name.is_empty() && !name.starts_with('@') && tgz(file),
        [scope, name, "-", file] => {
            scope.len() > 1 && scope.starts_with('@') && !name.is_empty() && tgz(file)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests;
