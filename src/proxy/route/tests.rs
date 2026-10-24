use rama::http::Method;

use super::*;

fn make_request(headers: &[(&str, &str)]) -> Request<Body> {
    make_request_at("/lodash", headers)
}

fn make_request_at(path: &str, headers: &[(&str, &str)]) -> Request<Body> {
    let mut builder = Request::builder().method(Method::GET).uri(path);

    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }

    builder.body(Body::empty()).unwrap()
}

#[test]
fn test_is_npm_request_with_npm_command() {
    let req = make_request(&[("npm-command", "install")]);
    assert!(is_npm_request(&req));
}

#[test]
fn test_is_npm_request_with_npm_user_agent() {
    let req = make_request(&[("user-agent", "npm/9.0.0 node/v18.0.0")]);
    assert!(is_npm_request(&req));
}

#[test]
fn test_is_npm_request_with_vnd_accept() {
    let req = make_request(&[("accept", "application/vnd.npm.install-v1+json")]);
    assert!(is_npm_request(&req));
}

#[test]
fn bun_is_recognized_on_metadata_and_tarball_requests() {
    // bun 1.4.3, verbatim: a download carries no npm header at all.
    const BUN_TARBALL: &[(&str, &str)] = &[("user-agent", "Bun/1.4.3"), ("accept", "*/*")];
    // (path, headers, recognized as npm)
    type Case = (&'static str, &'static [(&'static str, &'static str)], bool);
    const CASES: &[Case] = &[
        ("/is-odd/-/is-odd-3.0.1.tgz", BUN_TARBALL, true),
        ("/@babel/core/-/core-7.26.0.tgz", &[("accept", "*/*")], true),
        ("/is-odd", &[("user-agent", "Bun/1.4.3")], true),
        (
            "/is-odd",
            &[("user-agent", "pnpm/9.15.0 npm/? node/v22.0.0 darwin arm64")],
            true,
        ),
        // RubyGems and crates.io paths stay theirs.
        ("/gems/rake-13.4.2.gem", &[("accept", "*/*")], false),
        (
            "/api/v1/crates/itoa/1.0.18/download",
            &[("accept", "*/*")],
            false,
        ),
        ("/-/x.tgz", &[], false),
        ("/@/name/-/x.tgz", &[], false),
        ("/is-odd/-/is-odd-3.0.1.tgz.sha", &[], false),
    ];
    for (path, headers, expected) in CASES {
        assert_eq!(
            is_npm_request(&make_request_at(path, headers)),
            *expected,
            "{path} {headers:?}"
        );
    }
}

#[test]
fn test_is_npm_request_without_npm_headers() {
    let req = make_request(&[("user-agent", "curl/7.0.0")]);
    assert!(!is_npm_request(&req));
}

/// A request (path, headers) and the registry it is for.
type Case = (
    &'static str,
    &'static [(&'static str, &'static str)],
    Option<AssetKind>,
);

#[test]
fn requests_are_told_apart_by_registry() {
    const CASES: &[Case] = &[
        ("/gems/rake-13.4.2.gem", &[], Some(AssetKind::Gem)),
        ("/info/rack", &[], Some(AssetKind::Gem)),
        ("/versions", &[], Some(AssetKind::Gem)),
        ("/api/v1/search.json?query=rack", &[], Some(AssetKind::Gem)),
        (
            "/.well-known/vein/sbom?name=rack&version=3.2.4",
            &[],
            Some(AssetKind::Gem),
        ),
        ("/index/se/rd/serde", &[], Some(AssetKind::Crate)),
        (
            "/api/v1/crates/itoa/1.0.18/download",
            &[],
            Some(AssetKind::Crate),
        ),
        ("/api/v1/crates?q=serde", &[], Some(AssetKind::Crate)),
        (
            "/-/v1/search?text=left-pad",
            &[],
            Some(AssetKind::NpmPackage),
        ),
        (
            "/left-pad",
            &[("npm-command", "install")],
            Some(AssetKind::NpmPackage),
        ),
        (
            "/left-pad/-/left-pad-1.3.0.tgz",
            &[],
            Some(AssetKind::NpmPackage),
        ),
        ("/", &[], None),
        ("/up", &[], None),
        ("/docs/gems/rack/3.2.4", &[], None),
    ];
    for (path, headers, expected) in CASES {
        assert_eq!(
            registry_of(&make_request_at(path, headers)),
            *expected,
            "{path} {headers:?}"
        );
    }
}
