use std::sync::Arc;

use crate::config::Config as VeinConfig;
use rama::http::body::util::BodyExt;
use rama::http::{Body, Method, Request, StatusCode};
use serde_json::{Value, json};
use tempfile::TempDir;
use vein_adapter::{
    AssetKey, AssetKind, CacheBackendTrait, DependencyKind, FilesystemStorage, GemDependency,
};

use super::*;
use crate::admin::ruby::RubyStatus;

#[test]
fn loopback_binds_serve_without_a_token() {
    for host in ["127.0.0.1", "localhost", "::1", "[::1]"] {
        let config = transport_config(host, None).unwrap().expect(host);
        assert!(config.localhost_only, "{host}");
        assert!(config.bearer_auth.is_none(), "{host}");
    }
}

#[test]
fn public_binds_stay_off_without_a_token() {
    assert!(transport_config("0.0.0.0", None).unwrap().is_none());
    assert!(
        transport_config("192.168.1.10", Some(String::new()))
            .unwrap()
            .is_none()
    );
}

#[test]
fn public_binds_require_the_token() {
    let config = transport_config("0.0.0.0", Some("s3cret".into()))
        .unwrap()
        .unwrap();
    assert!(!config.localhost_only);
    assert!(config.bearer_auth.is_some());
}

/// Three sparse-index lines for `serde`; one JSON object per line.
const SERDE_INDEX: &str = concat!(
    r#"{"name":"serde","vers":"1.0.9","deps":[],"cksum":"aa","features":{},"yanked":false}"#,
    "\n",
    r#"{"name":"serde","vers":"1.0.10","#,
    r#""deps":[{"name":"serde_derive","req":"=1.0.10","features":[],"optional":true,"#,
    r#""default_features":true,"target":null,"kind":"normal"}],"#,
    r#""cksum":"bb","features":{"default":["std"],"std":[]},"#,
    r#""features2":{"derive":["dep:serde_derive"]},"yanked":false,"rust_version":"1.31"}"#,
    "\n",
    r#"{"name":"serde","vers":"1.0.11","deps":[],"cksum":"cc","features":{},"yanked":true}"#,
    "\n",
);

const TYPES_NODE_PACKUMENT: &str = r#"{
  "name": "@types/node",
  "description": "TypeScript definitions for node",
  "license": "MIT",
  "repository": {"type": "git", "url": "https://github.com/DefinitelyTyped/DefinitelyTyped.git"},
  "dist-tags": {"latest": "22.1.0"},
  "versions": {
    "22.0.0": {"deprecated": "use 22.1.0", "dependencies": {"undici-types": "~6.11.1"}},
    "22.1.0": {
      "license": "MIT",
      "dependencies": {"undici-types": "~6.13.0"},
      "dist": {"integrity": "sha512-abc"}
    }
  }
}"#;

async fn seeded_resources(storage: &TempDir) -> AdminResources {
    let index = storage.path().join("crates_index/se/rd");
    std::fs::create_dir_all(&index).unwrap();
    std::fs::write(index.join("serde"), SERDE_INDEX).unwrap();
    let packument = storage.path().join("npm_index/@types_node");
    std::fs::create_dir_all(&packument).unwrap();
    std::fs::write(packument.join("metadata.json"), TYPES_NODE_PACKUMENT).unwrap();

    let cache = vein_adapter::test_support::connect_test_backend()
        .await
        .unwrap();
    for (kind, name, version) in [
        (AssetKind::Crate, "serde", "1.0.9"),
        (AssetKind::Crate, "serde", "1.0.10"),
        (AssetKind::NpmPackage, "@types/node", "22.1.0"),
    ] {
        let key = AssetKey {
            kind,
            name,
            version,
            platform: None,
        };
        cache
            .insert_or_replace(&key, "path", "ff", 2)
            .await
            .unwrap();
    }
    // rubygems.org really has both `Rack` and `rack`.
    let names = ["Rack", "rack", "rack-test", "rails"].map(String::from);
    cache.catalog_upsert_names(&names).await.unwrap();
    for version in ["3.1.9", "3.1.10"] {
        let key = AssetKey {
            kind: AssetKind::Gem,
            name: "rack",
            version,
            platform: None,
        };
        cache
            .insert_or_replace(&key, &format!("gems/rack-{version}.gem"), "00", 1)
            .await
            .unwrap();
    }
    cache.upsert_metadata(&rack_metadata()).await.unwrap();

    AdminResources::new(
        Arc::new(VeinConfig::default()),
        Arc::new(cache),
        Arc::new(FilesystemStorage::new(storage.path().to_path_buf())),
        Arc::new(RubyStatus::default()),
    )
}

fn rack_metadata() -> GemMetadata {
    GemMetadata {
        name: "rack".into(),
        version: "3.1.10".into(),
        platform: "ruby".into(),
        summary: Some("A modular Ruby webserver interface.".into()),
        description: None,
        licenses: vec!["MIT".into()],
        authors: vec!["Leah Neukirchen".into()],
        emails: Vec::new(),
        homepage: Some("https://github.com/rack/rack".into()),
        documentation_url: None,
        changelog_url: None,
        source_code_url: None,
        bug_tracker_url: None,
        wiki_url: None,
        funding_url: None,
        metadata: json!({}),
        dependencies: vec![GemDependency {
            name: "minitest".into(),
            requirement: "~> 5.0".into(),
            kind: DependencyKind::Development,
        }],
        executables: Vec::new(),
        extensions: Vec::new(),
        native_languages: Vec::new(),
        has_native_extensions: false,
        has_embedded_binaries: false,
        required_ruby_version: Some(">= 2.4.0".into()),
        required_rubygems_version: None,
        rubygems_version: None,
        specification_version: None,
        built_at: None,
        size_bytes: 1,
        sha256: "00".into(),
        sbom: Some(json!({"bomFormat": "CycloneDX", "specVersion": "1.5"})),
    }
}

fn test_config() -> HttpTransportConfig {
    HttpTransportConfig {
        // Keep POST SSE bodies finite so tests can collect them.
        sse_keep_alive_interval: None,
        ..transport_config("127.0.0.1", None).unwrap().unwrap()
    }
}

struct Session<'a> {
    service: &'a McpHttpService,
    id: String,
    capabilities: Value,
}

impl Session<'_> {
    async fn open(service: &McpHttpService) -> Session<'_> {
        Self::open_as(service, "test", "1").await
    }

    /// Initialize as the client `clientInfo` named `name` at `version`.
    async fn open_as<'s>(service: &'s McpHttpService, name: &str, version: &str) -> Session<'s> {
        let response = service
            .handle(post(
                None,
                &json!({
                    "jsonrpc": "2.0", "id": 1, "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-11-25",
                        "capabilities": {},
                        "clientInfo": {"name": name, "version": version}
                    }
                }),
            ))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let id = response.headers()["Mcp-Session-Id"]
            .to_str()
            .unwrap()
            .to_string();
        let init = sse_response(response.into_body()).await;
        let session = Session {
            service,
            id,
            capabilities: init["result"]["capabilities"].clone(),
        };
        session
            .send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await;
        session
    }

    async fn send(&self, message: Value) -> Value {
        let response = self.service.handle(post(Some(&self.id), &message)).await;
        sse_response(response.into_body()).await
    }

    /// The JSON-RPC response frame for `method`.
    async fn rpc(&self, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc": "2.0", "id": 2, "method": method, "params": params}))
            .await
    }

    async fn result(&self, method: &str, params: Value) -> Value {
        let frame = self.rpc(method, params).await;
        assert!(frame.get("error").is_none(), "{method} failed: {frame}");
        frame["result"].clone()
    }

    /// Read `uri`, returning its MIME type and parsed JSON body.
    async fn read(&self, uri: &str) -> (String, Value) {
        let result = self.result("resources/read", json!({"uri": uri})).await;
        let content = &result["contents"][0];
        assert_eq!(content["uri"], uri);
        let body = serde_json::from_str(content["text"].as_str().unwrap()).unwrap();
        (content["mimeType"].as_str().unwrap().to_string(), body)
    }

    async fn complete(&self, template: &str, argument: &str, value: &str, context: Value) -> Value {
        let result = self
            .result(
                "completion/complete",
                json!({
                    "ref": {"type": "ref/resource", "uri": template},
                    "argument": {"name": argument, "value": value},
                    "context": {"arguments": context},
                }),
            )
            .await;
        result["completion"]["values"].clone()
    }
}

/// The response frame of an SSE (or plain JSON) body; empty priming events
/// are skipped.
async fn sse_response(body: Body) -> Value {
    let bytes = body.collect().await.unwrap().to_bytes();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    text.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .find(|frame| frame.get("id").is_some())
        .or_else(|| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

fn post(session_id: Option<&str>, body: &Value) -> Request {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri("/mcp")
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream");
    if let Some(id) = session_id {
        builder = builder.header("Mcp-Session-Id", id);
    }
    builder
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

/// The MCP service over seeded data; keep the `TempDir` alive while using it.
async fn open() -> (McpHttpService, TempDir) {
    let storage = tempfile::tempdir().unwrap();
    let service = service(seeded_resources(&storage).await, test_config()).unwrap();
    (service, storage)
}

#[tokio::test]
async fn advertises_resources_and_no_tools() {
    let (service, _storage) = open().await;
    let session = Session::open(&service).await;

    assert!(session.capabilities.get("tools").is_none());
    assert!(session.capabilities.get("resources").is_some());
    assert!(session.capabilities.get("completions").is_some());

    let listed = session.result("resources/list", json!({})).await;
    let mut uris: Vec<&str> = listed["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|resource| resource["uri"].as_str().unwrap())
        .collect();
    uris.sort_unstable();
    assert_eq!(uris, ["vein://quarantine", "vein://stats"]);

    let listed = session.result("resources/templates/list", json!({})).await;
    let mut listed: Vec<&str> = listed["resourceTemplates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|template| template["uriTemplate"].as_str().unwrap())
        .collect();
    listed.sort_unstable();
    // The completion registrations key on these constants; keep them in sync.
    let mut expected = [
        templates::GEM_SEARCH,
        templates::GEM,
        templates::GEM_VERSION,
        templates::GEM_SBOM,
        templates::CRATE_SEARCH,
        templates::CRATE,
        templates::CRATE_VERSION,
        templates::NPM_SEARCH,
        templates::NPM_PACKAGE,
        templates::NPM_VERSION,
    ];
    expected.sort_unstable();
    assert_eq!(listed, expected);
}

#[tokio::test]
async fn gems_are_readable_as_resources() {
    let (service, _storage) = open().await;
    let session = Session::open(&service).await;

    let (_, search) = session.read("vein://gems/search/rack").await;
    assert_eq!(search["names"], json!(["rack", "Rack", "rack-test"]));
    assert_eq!(search["scope"], "rubygems.org");
    assert_eq!(search["catalog_size"], 4);

    let (_, gem) = session.read("vein://gems/rack").await;
    assert_eq!(gem["in_catalog"], true);
    assert_eq!(
        gem["cached_versions"],
        json!([
            "vein://gems/rack/versions/3.1.10",
            "vein://gems/rack/versions/3.1.9"
        ])
    );
    assert_eq!(gem["latest"]["version"], "3.1.10");

    let (mime, version) = session.read("vein://gems/rack/versions/3.1.10").await;
    assert_eq!(mime, "application/json");
    assert_eq!(version["licenses"], json!(["MIT"]));
    assert_eq!(version["dependencies"][0]["kind"], "development");
    assert_eq!(version["sbom"], "vein://gems/rack/versions/3.1.10/sbom");

    let (mime, sbom) = session.read("vein://gems/rack/versions/3.1.10/sbom").await;
    assert_eq!(mime, "application/vnd.cyclonedx+json");
    assert_eq!(sbom["bomFormat"], "CycloneDX");

    let (_, stats) = session.read("vein://stats").await;
    assert_eq!(stats["rubygems_assets"], 2);
    assert_eq!(stats["gem_versions_with_sbom"], 1);

    let (_, quarantine) = session.read("vein://quarantine").await;
    assert_eq!(quarantine["enabled"], false);
}

#[tokio::test]
async fn missing_versions_and_platforms_are_not_found() {
    let (service, _storage) = open().await;
    let session = Session::open(&service).await;

    let frame = session
        .rpc(
            "resources/read",
            json!({"uri": "vein://gems/rack/versions/9.9.9"}),
        )
        .await;
    let message = frame["error"]["message"].to_string();
    assert!(message.contains("rack 9.9.9 is not cached"), "{frame}");
    assert!(message.contains("3.1.10, 3.1.9"), "{frame}");

    let frame = session
        .rpc(
            "resources/read",
            json!({"uri": "vein://gems/rack/versions/3.1.10-java"}),
        )
        .await;
    let message = frame["error"]["message"].to_string();
    assert!(message.contains("rack 3.1.10 (java)"), "{frame}");
}

/// Claude Code before 2.93 hides JSON-RPC errors from the model, so read
/// failures (other than "not found") are answered as text for it alone.
#[tokio::test]
async fn read_errors_reach_old_claude_code_as_text() {
    const CASES: &[(&str, &str, bool)] = &[
        ("claude-code", "2.92.1", true),
        ("claude-code", "2.93.0", false),
        ("test", "1", false),
    ];
    // 65 characters: well-formed URI, but over the crates.io name limit.
    let uri = format!("vein://crates/{}", "a".repeat(65));

    let (service, _storage) = open().await;
    for (client, version, as_text) in CASES {
        let session = Session::open_as(&service, client, version).await;
        let frame = session.rpc("resources/read", json!({"uri": uri})).await;
        if *as_text {
            let content = &frame["result"]["contents"][0];
            assert_eq!(
                content["mimeType"], "text/plain",
                "{client} {version}: {frame}"
            );
            let text = content["text"].as_str().unwrap();
            assert!(
                text.starts_with(&format!("Error reading {uri}:"))
                    && text.contains("not a crate name"),
                "{client} {version}: {frame}"
            );
        } else {
            let message = frame["error"]["message"].to_string();
            assert!(
                message.contains("not a crate name"),
                "{client} {version}: {frame}"
            );
        }
    }
}

#[tokio::test]
async fn completes_gem_names_and_cached_versions() {
    let (service, _storage) = open().await;
    let session = Session::open(&service).await;

    let names = session
        .complete("vein://gems/{name}", "name", "ra", json!({}))
        .await;
    assert_eq!(names, json!(["Rack", "rack", "rack-test", "rails"]));

    let versions = session
        .complete(
            "vein://gems/{name}/versions/{version}",
            "version",
            "3.1.1",
            json!({"name": "rack"}),
        )
        .await;
    assert_eq!(versions, json!(["3.1.10"]));
}

#[tokio::test]
async fn crates_are_readable_as_resources() {
    let (service, _storage) = open().await;
    let session = Session::open(&service).await;

    let (_, search) = session.read("vein://crates/search/ser").await;
    assert_eq!(search["names"], json!(["serde"]));
    assert_eq!(search["scope"], "cached");

    let (_, serde) = session.read("vein://crates/serde").await;
    assert_eq!(serde["latest"], "1.0.10", "1.0.11 is yanked");
    assert_eq!(serde["total_versions"], 3);
    assert_eq!(serde["versions"][0]["version"], "1.0.11");
    assert_eq!(serde["versions"][0]["yanked"], true);
    let cached: Vec<&str> = serde["cached"]
        .as_array()
        .unwrap()
        .iter()
        .map(|artifact| artifact["version"].as_str().unwrap())
        .collect();
    assert_eq!(cached, ["1.0.10", "1.0.9"]);

    let (_, version) = session.read("vein://crates/serde/versions/1.0.10").await;
    assert_eq!(version["cached"]["sha256"], "ff");
    assert_eq!(version["published"]["checksum"], "bb");
    assert_eq!(version["published"]["rust_version"], "1.31");
    assert_eq!(
        version["published"]["features"]["derive"],
        json!(["dep:serde_derive"])
    );
    assert_eq!(
        version["published"]["dependencies"][0]["name"],
        "serde_derive"
    );
    assert_eq!(version["published"]["dependencies"][0]["optional"], true);

    let (_, yanked) = session.read("vein://crates/serde/versions/1.0.11").await;
    assert_eq!(yanked["cached"], Value::Null);
    assert_eq!(yanked["published"]["yanked"], true);

    let frame = session
        .rpc("resources/read", json!({"uri": "vein://crates/tokio"}))
        .await;
    assert!(
        frame["error"]["message"]
            .to_string()
            .contains("has not been fetched")
    );
    let frame = session
        .rpc("resources/read", json!({"uri": "vein://crates/...."}))
        .await;
    assert!(
        frame["error"]["message"]
            .to_string()
            .contains("not a crate name")
    );
}

#[tokio::test]
async fn npm_packages_are_readable_as_resources() {
    let (service, _storage) = open().await;
    let session = Session::open(&service).await;

    let (_, search) = session.read("vein://npm/search/types").await;
    assert_eq!(search["names"], json!(["@types/node"]));

    let (_, package) = session.read("vein://npm/@types/node").await;
    assert_eq!(package["license"], "MIT");
    assert_eq!(
        package["repository"],
        "https://github.com/DefinitelyTyped/DefinitelyTyped.git"
    );
    assert_eq!(package["dist_tags"]["latest"], "22.1.0");
    assert_eq!(package["versions"][0]["version"], "22.1.0");
    assert_eq!(package["versions"][1]["deprecated"], true);
    assert_eq!(package["cached"][0]["version"], "22.1.0");

    let (_, version) = session.read("vein://npm/@types/node/versions/22.1.0").await;
    assert_eq!(
        version["published"]["dependencies"]["undici-types"],
        "~6.13.0"
    );
    assert_eq!(version["published"]["integrity"], "sha512-abc");
    assert_eq!(version["cached"]["size_bytes"], 2);

    let (_, deprecated) = session.read("vein://npm/@types/node/versions/22.0.0").await;
    assert_eq!(deprecated["published"]["deprecated"], "use 22.1.0");
    assert_eq!(deprecated["cached"], Value::Null);

    let frame = session
        .rpc("resources/read", json!({"uri": "vein://npm/left-pad"}))
        .await;
    assert!(
        frame["error"]["message"]
            .to_string()
            .contains("has not been fetched")
    );
}

#[tokio::test]
async fn completes_crate_and_npm_arguments_from_the_cache() {
    let (service, _storage) = open().await;
    let session = Session::open(&service).await;

    let names = session
        .complete(templates::CRATE, "name", "se", json!({}))
        .await;
    assert_eq!(names, json!(["serde"]));

    let versions = session
        .complete(
            templates::CRATE_VERSION,
            "version",
            "",
            json!({"name": "serde"}),
        )
        .await;
    assert_eq!(versions, json!(["1.0.10", "1.0.9"]));

    let names = session
        .complete(templates::NPM_PACKAGE, "name", "@types", json!({}))
        .await;
    assert_eq!(names, json!(["@types/node"]));
}
