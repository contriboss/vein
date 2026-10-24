use std::sync::Arc;

use mcp_host::prelude::{ExecutionContext, Server, Tool, ToolError, ToolFuture, ToolOutput};
use mcp_host::protocol::types::ToolAnnotations;
use mcp_host::transport::http::{HttpBearerAuthConfig, HttpTransportConfig, McpHttpService};
use rama::http::server::HttpServer;
use rama::rt::Executor;
use rama::tcp::server::TcpListener;
use serde_json::json;

use super::*;

const TOKEN: &str = "ghi-test-token";

/// MOCKED: `GitHits`' `search`, answering the way the real one does: a
/// result naming its follow-up, or an error for an unknown package.
struct Search;

impl Tool for Search {
    fn name(&self) -> &'static str {
        "search"
    }

    fn description(&self) -> Option<&str> {
        Some("Discover relevant docs, code, and symbols in a known public target.")
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"query": {"type": "string"}, "target": {"type": "string"}},
            "required": ["query"],
        })
    }

    fn annotations(&self) -> Option<ToolAnnotations> {
        Some(ToolAnnotations {
            read_only_hint: Some(true),
            open_world_hint: Some(true),
            ..ToolAnnotations::default()
        })
    }

    fn execute<'a>(&'a self, ctx: ExecutionContext<'a>) -> ToolFuture<'a> {
        Box::pin(async move {
            let target = ctx.params["target"].as_str().unwrap_or_default().to_owned();
            if target == "crates:no-such-crate" {
                return Err(ToolError::Execution(
                    "Package \"no-such-crate\" was not found in the crates registry.".into(),
                ));
            }
            Ok(ToolOutput::text(format!(
                "[1] {target} state-machines-core/src/lib.rs:152-156 [repo code]\n\
                 Read:\n  read target=\"{target}\" path=\"state-machines-core/src/lib.rs\""
            )))
        })
    }
}

/// MOCKED: `GitHits`' hosted MCP on a loopback port, behind bearer auth.
async fn fake_githits() -> String {
    crate::test_support::install_rustls_provider();
    let server = Server::builder("githits", "0.1.0")
        .with_tools(false)
        .build();
    server.tool_registry().register(Search).unwrap();
    let config = HttpTransportConfig {
        localhost_only: true,
        bearer_auth: Some(HttpBearerAuthConfig::new(TOKEN, "vein").unwrap().into()),
        sse_keep_alive_interval: None,
        ..HttpTransportConfig::default()
    };
    let service = McpHttpService::new(Arc::new(server), config).unwrap();
    let listener = TcpListener::build(Executor::default())
        .bind_address("127.0.0.1:0")
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(listener.serve(HttpServer::auto(Executor::default()).service(Arc::new(service))));
    format!("http://127.0.0.1:{port}/mcp")
}

fn githits(url: String, token: &str) -> GitHits {
    GitHits {
        url,
        token: token.to_owned(),
        session: Mutex::new(None),
    }
}

#[tokio::test]
async fn its_tools_and_answers_come_over_its_mcp() {
    let githits = githits(fake_githits().await, TOKEN);
    let tools = githits.tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    let search = &tools[0];
    assert_eq!(search.name, "search");
    assert!(search.read_only && search.open_world);
    assert_eq!(search.input_schema["required"], json!(["query"]));

    let arguments = |target: &str| {
        let mut arguments = Map::new();
        arguments.insert("query".into(), "GuardError".into());
        arguments.insert("target".into(), target.into());
        arguments
    };
    let found = githits
        .call("search", arguments("crates:state-machines@0.32.1"))
        .await
        .unwrap();
    assert!(!found.is_error);
    assert!(
        found
            .text
            .contains("read target=\"crates:state-machines@0.32.1\""),
        "{}",
        found.text
    );
    let missing = githits
        .call("search", arguments("crates:no-such-crate"))
        .await
        .unwrap();
    assert!(missing.is_error);
    assert!(missing.text.contains("was not found"), "{}", missing.text);
}

#[tokio::test]
async fn a_wrong_token_is_refused() {
    let err = githits(fake_githits().await, "ghi-wrong")
        .tools()
        .await
        .unwrap_err();
    assert!(
        format!("{err:#}").contains("connecting to GitHits"),
        "{err:#}"
    );
}

#[test]
fn only_a_lost_session_is_retried() {
    for (err, expected) in [
        (GuestError::SessionExpired, true),
        (GuestError::Disconnected, true),
        (GuestError::Http("connection reset".into()), true),
        (GuestError::Timeout(Duration::from_secs(150)), false),
        (GuestError::Cancelled, false),
        (GuestError::InvalidParams("query".into()), false),
    ] {
        assert_eq!(lost(&err), expected, "{err}");
    }
}

#[test]
fn answers_keep_text_and_name_the_rest() {
    let result: CallToolResult = serde_json::from_value(json!({
        "content": [
            {"type": "text", "text": "Found 1 result."},
            {"type": "image", "data": "aGk=", "mimeType": "image/png"},
            {"type": "text", "text": "Read: read target=\"npm:react\""},
        ],
    }))
    .unwrap();
    assert_eq!(
        answer(result),
        Answer {
            text: "Found 1 result.\n\n[image content not shown]\n\nRead: read target=\"npm:react\""
                .into(),
            is_error: false,
        }
    );
    let structured: CallToolResult =
        serde_json::from_value(json!({"structuredContent": {"hits": 0}, "isError": true})).unwrap();
    assert_eq!(
        answer(structured),
        Answer {
            text: "{\"hits\":0}".into(),
            is_error: true,
        }
    );
}
