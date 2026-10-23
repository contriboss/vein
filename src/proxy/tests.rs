use super::*;

use crate::test_support::{install_rustls_provider, raw_response, spawn_sequence_server};

use crate::config::Config;
use rama::http::{Body, Method, Request};
use rama::net::{Protocol, uri::Uri};
use rama::{Service, http::body::util::BodyExt};
use std::{path::Path, sync::Arc};
use tempfile::tempdir;
use types::{CacheStatus, CacheableRequest, UpstreamTarget};
use vein_adapter::AssetKind;
use vein_adapter::FilesystemStorage;

fn req(path: &str) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

#[test]
fn from_gem_path_simple() {
    let result = CacheableRequest::from_gem_path("rack-3.0.0.gem").unwrap();
    assert_eq!(result.kind, AssetKind::Gem);
    assert_eq!(result.name, "rack");
    assert_eq!(result.version, "3.0.0");
    assert!(result.platform.is_none());
    assert_eq!(result.file_name, "rack-3.0.0.gem");
    assert_eq!(result.relative_path, "gems/rack/rack-3.0.0.gem");
}

#[test]
fn from_gem_path_with_platform() {
    let result = CacheableRequest::from_gem_path("nokogiri-1.15.5-x86_64-darwin.gem").unwrap();
    assert_eq!(result.kind, AssetKind::Gem);
    assert_eq!(result.name, "nokogiri");
    assert_eq!(result.version, "1.15.5");
    assert_eq!(result.platform.as_deref(), Some("x86_64-darwin"));
    assert_eq!(result.file_name, "nokogiri-1.15.5-x86_64-darwin.gem");
    assert_eq!(
        result.relative_path,
        "gems/nokogiri/nokogiri-1.15.5-x86_64-darwin.gem"
    );
}

#[test]
fn from_gem_path_hyphenated_name() {
    let result = CacheableRequest::from_gem_path("active-support-7.1.0.gem").unwrap();
    assert_eq!(result.name, "active-support");
    assert_eq!(result.version, "7.1.0");
}

#[test]
fn from_gem_path_rejects_no_gem_extension() {
    assert!(CacheableRequest::from_gem_path("rack-3.0.0").is_none());
}

#[test]
fn from_gem_path_rejects_invalid_format() {
    assert!(CacheableRequest::from_gem_path("invalid.gem").is_none());
}

#[test]
fn from_gem_path_java_platform() {
    let result = CacheableRequest::from_gem_path("jruby-rack-1.1.21-java.gem").unwrap();
    assert_eq!(result.name, "jruby-rack");
    assert_eq!(result.version, "1.1.21");
    assert_eq!(result.platform.as_deref(), Some("java"));
}

#[test]
fn from_spec_path_simple() {
    let result = CacheableRequest::from_spec_path("rack-3.0.0.gemspec.rz").unwrap();
    assert_eq!(result.kind, AssetKind::Spec);
    assert_eq!(result.name, "rack");
    assert_eq!(result.version, "3.0.0");
    assert!(result.platform.is_none());
    assert_eq!(result.file_name, "rack-3.0.0.gemspec.rz");
    assert_eq!(
        result.relative_path,
        "quick/Marshal.4.8/rack/rack-3.0.0.gemspec.rz"
    );
}

#[test]
fn from_spec_path_with_platform() {
    let result =
        CacheableRequest::from_spec_path("nokogiri-1.15.5-x86_64-linux.gemspec.rz").unwrap();
    assert_eq!(result.kind, AssetKind::Spec);
    assert_eq!(result.name, "nokogiri");
    assert_eq!(result.version, "1.15.5");
    assert_eq!(result.platform.as_deref(), Some("x86_64-linux"));
}

#[test]
fn from_spec_path_rejects_wrong_extension() {
    assert!(CacheableRequest::from_spec_path("rack-3.0.0.gem").is_none());
}

#[test]
fn from_spec_path_rejects_invalid_format() {
    assert!(CacheableRequest::from_spec_path("invalid.gemspec.rz").is_none());
}

#[test]
fn from_request_gem_path() {
    let result = CacheableRequest::from_request(&req("/gems/rack-3.0.0.gem")).unwrap();
    assert_eq!(result.kind, AssetKind::Gem);
    assert_eq!(result.name, "rack");
    assert_eq!(result.version, "3.0.0");
}

#[test]
fn from_request_spec_path() {
    let result =
        CacheableRequest::from_request(&req("/quick/Marshal.4.8/rack-3.0.0.gemspec.rz")).unwrap();
    assert_eq!(result.kind, AssetKind::Spec);
    assert_eq!(result.name, "rack");
}

#[test]
fn from_request_non_cacheable_path() {
    assert!(CacheableRequest::from_request(&req("/versions")).is_none());
}

#[test]
fn from_request_health_check() {
    assert!(CacheableRequest::from_request(&req("/up")).is_none());
}

#[test]
fn cacheable_request_download_name() {
    let req = CacheableRequest::from_gem_path("rack-3.0.0.gem").unwrap();
    assert_eq!(req.download_name(), "rack-3.0.0.gem");
}

#[test]
fn cacheable_request_content_type_gem() {
    let req = CacheableRequest::from_gem_path("rack-3.0.0.gem").unwrap();
    assert_eq!(req.content_type(), "application/octet-stream");
}

#[test]
fn cacheable_request_content_type_spec() {
    let req = CacheableRequest::from_spec_path("rack-3.0.0.gemspec.rz").unwrap();
    assert_eq!(req.content_type(), "application/x-deflate");
}

#[test]
fn cacheable_request_asset_key_no_platform() {
    let req = CacheableRequest::from_gem_path("rack-3.0.0.gem").unwrap();
    let key = req.asset_key();
    assert_eq!(key.kind, AssetKind::Gem);
    assert_eq!(key.name, "rack");
    assert_eq!(key.version, "3.0.0");
    assert!(key.platform.is_none());
}

#[test]
fn cacheable_request_asset_key_with_platform() {
    let req = CacheableRequest::from_gem_path("nokogiri-1.15.5-x86_64-darwin.gem").unwrap();
    let key = req.asset_key();
    assert_eq!(key.platform, Some("x86_64-darwin"));
}

#[test]
fn upstream_from_url_https_default_port() {
    let url = Uri::from_static("https://rubygems.org");
    let upstream = UpstreamTarget::from_url(&url).unwrap();
    assert_eq!(upstream.base.host_str().as_deref(), Some("rubygems.org"));
    assert_eq!(upstream.base.scheme(), Some(&Protocol::HTTPS));
}

#[test]
fn upstream_from_url_https_custom_port() {
    let url = Uri::from_static("https://example.com:8443");
    let upstream = UpstreamTarget::from_url(&url).unwrap();
    assert_eq!(upstream.base.host_str().as_deref(), Some("example.com"));
}

#[test]
fn upstream_from_url_http_default_port() {
    let url = Uri::from_static("http://localhost");
    let upstream = UpstreamTarget::from_url(&url).unwrap();
    assert_eq!(upstream.base.host_str().as_deref(), Some("localhost"));
    assert_eq!(upstream.base.scheme(), Some(&Protocol::HTTP));
}

#[test]
fn upstream_from_url_http_custom_port() {
    let url = Uri::from_static("http://localhost:3000");
    let upstream = UpstreamTarget::from_url(&url).unwrap();
    assert_eq!(upstream.base.port_u16(), Some(3000));
    assert_eq!(upstream.base.scheme(), Some(&Protocol::HTTP));
}

#[test]
fn upstream_from_url_with_path() {
    let url = Uri::from_static("https://rubygems.org/api");
    let upstream = UpstreamTarget::from_url(&url).unwrap();
    assert_eq!(upstream.base.to_string(), "https://rubygems.org/api");
}

#[test]
fn upstream_join_simple_path() {
    let url = Uri::from_static("https://rubygems.org");
    let upstream = UpstreamTarget::from_url(&url).unwrap();
    let result = upstream.join(&req("/gems/rack-3.0.0.gem")).unwrap();
    assert_eq!(
        result.to_string(),
        "https://rubygems.org/gems/rack-3.0.0.gem"
    );
}

#[test]
fn upstream_join_with_base_path() {
    let url = Uri::from_static("https://example.com/api/v1/");
    let upstream = UpstreamTarget::from_url(&url).unwrap();
    let result = upstream.join(&req("/gems/rack-3.0.0.gem")).unwrap();
    assert_eq!(
        result.to_string(),
        "https://example.com/api/v1/gems/rack-3.0.0.gem"
    );
}

#[test]
fn upstream_join_root_path() {
    let url = Uri::from_static("https://rubygems.org");
    let upstream = UpstreamTarget::from_url(&url).unwrap();
    let result = upstream.join(&req("/")).unwrap();
    assert_eq!(result.to_string(), "https://rubygems.org/");
}

#[test]
fn upstream_join_with_query_string() {
    let url = Uri::from_static("https://rubygems.org");
    let upstream = UpstreamTarget::from_url(&url).unwrap();
    let result = upstream
        .join(&req("/api/v1/dependencies?gems=rack"))
        .unwrap();
    assert_eq!(
        result.to_string(),
        "https://rubygems.org/api/v1/dependencies?gems=rack"
    );
}

#[test]
fn upstream_join_no_leading_slash_still_works() {
    let url = Uri::from_static("https://rubygems.org/");
    let upstream = UpstreamTarget::from_url(&url).unwrap();
    let result = upstream.join(&req("/gems/rack-3.0.0.gem")).unwrap();
    assert!(result.to_string().contains("/gems/rack-3.0.0.gem"));
}

#[test]
fn cache_status_display_pass() {
    assert_eq!(CacheStatus::Pass.to_string(), "pass");
}

#[test]
fn cache_status_display_hit() {
    assert_eq!(CacheStatus::Hit.to_string(), "hit");
}

#[test]
fn cache_status_display_miss() {
    assert_eq!(CacheStatus::Miss.to_string(), "miss");
}

#[test]
fn cache_status_display_revalidated() {
    assert_eq!(CacheStatus::Revalidated.to_string(), "revalidated");
}

#[test]
fn cache_status_display_error() {
    assert_eq!(CacheStatus::Error.to_string(), "error");
}

#[test]
fn cache_status_equality() {
    assert_eq!(CacheStatus::Hit, CacheStatus::Hit);
    assert_ne!(CacheStatus::Hit, CacheStatus::Miss);
}

#[test]
fn request_context_default() {
    let ctx = RequestContext::default();
    assert_eq!(ctx.method, Method::GET);
    assert_eq!(ctx.path, "");
    assert_eq!(ctx.cache, CacheStatus::Pass);
    assert!(ctx.start.elapsed().as_millis() < 100);
}

#[test]
fn from_gem_path_empty_string() {
    assert!(CacheableRequest::from_gem_path("").is_none());
}

#[test]
fn from_spec_path_empty_string() {
    assert!(CacheableRequest::from_spec_path("").is_none());
}

#[test]
fn from_gem_path_just_extension() {
    assert!(CacheableRequest::from_gem_path(".gem").is_none());
}

#[test]
fn from_spec_path_just_extension() {
    assert!(CacheableRequest::from_spec_path(".gemspec.rz").is_none());
}

#[test]
fn upstream_join_handles_special_chars() {
    let url = Uri::from_static("https://rubygems.org");
    let upstream = UpstreamTarget::from_url(&url).unwrap();
    let result = upstream.join(&req("/info/my%2Dgem")).unwrap();
    assert!(result.to_string().contains("my%2Dgem"));
}

#[test]
fn cacheable_request_relative_path_structure_gem() {
    let req = CacheableRequest::from_gem_path("rails-7.1.0.gem").unwrap();
    assert_eq!(req.relative_path, "gems/rails/rails-7.1.0.gem");
}

#[test]
fn cacheable_request_relative_path_structure_spec() {
    let req = CacheableRequest::from_spec_path("rails-7.1.0.gemspec.rz").unwrap();
    assert_eq!(
        req.relative_path,
        "quick/Marshal.4.8/rails/rails-7.1.0.gemspec.rz"
    );
}

#[tokio::test]
async fn proxy_routes_npm_requests_by_header() {
    install_rustls_provider();

    let temp_dir = tempdir().unwrap();
    let proxy = build_test_proxy(temp_dir.path()).await;

    let metadata = br#"{
        "name": "lodash",
        "versions": {
            "4.17.21": {
                "dist": {
                    "tarball": "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz"
                }
            }
        }
    }"#;
    let (registry_base, server) = spawn_sequence_server(vec![
        raw_response(
            "200 OK",
            &[
                ("Content-Type", "application/json"),
                ("ETag", "\"lodash-meta-v1\""),
                ("Last-Modified", "Wed, 01 Jan 2025 00:00:00 GMT"),
            ],
            metadata,
        ),
        raw_response("304 Not Modified", &[], &[]),
    ])
    .await;
    let _guard = crate::npm::override_npm_registry_base(registry_base);

    let response = proxy
        .serve(req_with_headers("/lodash", &[("npm-command", "view")]))
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let body: serde_json::Value = serde_json::from_slice(&body_bytes(response).await).unwrap();
    assert_eq!(
        body["versions"]["4.17.21"]["dist"]["tarball"]
            .as_str()
            .unwrap(),
        "http://127.0.0.1:8346/lodash/-/lodash-4.17.21.tgz"
    );

    let response = proxy
        .serve(req_with_headers("/lodash", &[("npm-command", "view")]))
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let body: serde_json::Value = serde_json::from_slice(&body_bytes(response).await).unwrap();
    assert_eq!(
        body["versions"]["4.17.21"]["dist"]["tarball"]
            .as_str()
            .unwrap(),
        "http://127.0.0.1:8346/lodash/-/lodash-4.17.21.tgz"
    );

    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("get /lodash http/1.1"));
    assert!(requests[1].contains("if-none-match: \"lodash-meta-v1\""));
}

#[tokio::test]
async fn proxy_does_not_treat_plain_requests_as_npm() {
    let temp_dir = tempdir().unwrap();
    let proxy = build_test_proxy(temp_dir.path()).await;

    let response = proxy.serve(req("/lodash")).await.unwrap();
    assert_eq!(response.status().as_u16(), 404);
    assert_eq!(body_bytes(response).await, b"not found in cache");
}

#[tokio::test]
async fn proxy_routes_crates_index_requests_by_path() {
    install_rustls_provider();

    let temp_dir = tempdir().unwrap();
    let proxy = build_test_proxy(temp_dir.path()).await;

    let body = b"{\"name\":\"serde\"}\n";
    let (index_base, server) = spawn_sequence_server(vec![
        raw_response(
            "200 OK",
            &[
                ("Content-Type", "text/plain"),
                ("ETag", "\"serde-v1\""),
                ("Last-Modified", "Wed, 01 Jan 2025 00:00:00 GMT"),
            ],
            body,
        ),
        raw_response("304 Not Modified", &[], &[]),
    ])
    .await;
    let _guard = crate::crates::override_crates_index_base(index_base);

    let response = proxy.serve(req("/index/se/rd/serde")).await.unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(body_bytes(response).await, body);

    let response = proxy.serve(req("/index/se/rd/serde")).await.unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(body_bytes(response).await, body);

    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("get /se/rd/serde http/1.1"));
    assert!(requests[1].contains("if-none-match: \"serde-v1\""));
}

async fn build_test_proxy(root: &Path) -> VeinProxy {
    let mut config = Config::default();
    config.server.host = "127.0.0.1".to_string();
    config.server.port = 8346;
    config.storage.path = root.join("cache");

    let storage = Arc::new(FilesystemStorage::new(config.storage.path.clone()));
    storage.prepare().await.unwrap();

    let index = Arc::new(
        vein_adapter::test_support::connect_test_backend()
            .await
            .unwrap(),
    );

    VeinProxy::new(Arc::new(config), storage, index).unwrap()
}

fn req_with_headers(path: &str, headers: &[(&str, &str)]) -> Request<Body> {
    let mut builder = Request::builder().method(Method::GET).uri(path);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(Body::empty()).unwrap()
}

async fn body_bytes(response: rama::http::Response<Body>) -> Vec<u8> {
    response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec()
}

#[tokio::test]
async fn homepage_shows_setup_for_the_url_the_client_used() {
    let mut config = Config::default();
    config.server.host = "127.0.0.1".to_string();
    config.storage.path = "/srv/vein <gems>".into();

    let response = response::respond_homepage(&config, "http://192.168.1.20:8080").unwrap();
    let cache = response
        .headers()
        .get(rama::http::header::CACHE_CONTROL)
        .cloned();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(body.to_vec()).unwrap();

    for snippet in [
        "bundle config set --global mirror.https://rubygems.org http://192.168.1.20:8080",
        "BUNDLE_MIRROR__HTTPS://RUBYGEMS__ORG/=http://192.168.1.20:8080",
        "registry = \"sparse+http://192.168.1.20:8080/index/\"",
        "index = \"sparse+http://192.168.1.20:8080/index/\"",
        "npm config set registry http://192.168.1.20:8080/",
        "BUN_CONFIG_REGISTRY=http://192.168.1.20:8080/",
    ] {
        assert!(html.contains(snippet), "missing {snippet:?}");
    }
    // The bind address is not what clients use; the path is escaped.
    assert!(!html.contains("127.0.0.1"), "{html}");
    assert!(html.contains("/srv/vein &lt;gems&gt;"));
    assert_eq!(cache.unwrap(), "no-cache, no-store, must-revalidate");
}
