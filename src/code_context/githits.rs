//! `GitHits` (<https://githits.com>) over its hosted MCP: vein is a client
//! of it, one session for the whole process, the API token as its bearer.

use std::future::Future;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use mcp_guest::{CallToolResult, ContentBlock, GuestError, McpSession, TaskOrResult, ToolInfo};
use rama::telemetry::tracing::warn;
use serde_json::{Map, Value};
use tokio::sync::Mutex;

use super::{Answer, Pending, Provider, ToolSpec};
use crate::config::code_context::GitHitsConfig;

/// `GitHits`' searches wait up to two minutes for a target's index.
const CALL_TIMEOUT: Duration = Duration::from_secs(150);

pub struct GitHits {
    url: String,
    token: String,
    session: Mutex<Option<McpSession>>,
}

impl GitHits {
    /// Fails without a token; connects on first use.
    pub fn new(config: &GitHitsConfig) -> Result<Self> {
        Ok(Self {
            url: config.url.clone(),
            token: config.token()?,
            session: Mutex::new(None),
        })
    }

    /// The session, opened first when there is none.
    async fn session(&self) -> Result<McpSession> {
        let mut held = self.session.lock().await;
        if let Some(session) = held.as_ref() {
            return Ok(session.clone());
        }
        let session = mcp_guest::http(&self.url)
            .bearer_token(self.token.clone())
            .client_name("vein")
            .client_version(env!("CARGO_PKG_VERSION"))
            .request_timeout(CALL_TIMEOUT)
            .connect()
            .await
            .with_context(|| format!("connecting to GitHits at {}", self.url))?;
        *held = Some(session.clone());
        Ok(session)
    }

    /// Drops the session, so the next request opens another.
    async fn forget(&self) {
        if let Some(session) = self.session.lock().await.take()
            && let Err(err) = session.disconnect().await
        {
            warn!(error = %err, "closing the lost GitHits session");
        }
    }

    /// `request` on the session, then once more on a new one if `GitHits`
    /// dropped it (a restart, an expired session).
    async fn ask<T, F, Fut>(&self, request: F) -> Result<T>
    where
        F: Fn(McpSession) -> Fut + Sync,
        Fut: Future<Output = Result<T, GuestError>> + Send,
        T: Send,
    {
        match request(self.session().await?).await {
            Ok(answer) => Ok(answer),
            Err(err) if lost(&err) => {
                warn!(error = %err, "GitHits session lost, opening another");
                self.forget().await;
                request(self.session().await?)
                    .await
                    .context("asking GitHits")
            }
            Err(err) => Err(err).context("asking GitHits"),
        }
    }
}

/// The session is gone, rather than the request wrong or slow.
const fn lost(err: &GuestError) -> bool {
    matches!(
        err,
        GuestError::SessionExpired
            | GuestError::Disconnected
            | GuestError::Transport(_)
            | GuestError::Http(_)
    )
}

impl Provider for GitHits {
    fn name(&self) -> &'static str {
        "GitHits"
    }

    fn about(&self) -> &'static str {
        "It indexes public open-source code and docs beyond this cache: search, read, \
         list and grep inside a package or repository (targets like \
         crates:serde@1.0.228, npm:react, github:owner/repo), examples and research from \
         other projects, package health and advisories."
    }

    fn remote(&self) -> bool {
        true
    }

    fn tools(&self) -> Pending<'_, Vec<ToolSpec>> {
        Box::pin(async move {
            let tools = self
                .ask(|session: McpSession| async move { session.list_tools().await })
                .await?;
            Ok(tools.into_iter().map(spec).collect())
        })
    }

    fn call<'a>(&'a self, name: &'a str, arguments: Map<String, Value>) -> Pending<'a, Answer> {
        Box::pin(async move {
            let response = self
                .ask(|session: McpSession| {
                    let arguments = arguments.clone();
                    async move { session.call_tool(name, Some(arguments)).await }
                })
                .await?;
            match response {
                TaskOrResult::Result(result) => Ok(answer(result)),
                TaskOrResult::Task(_) => {
                    bail!("GitHits answered {name} with a task, which vein never asks for")
                }
            }
        })
    }
}

fn spec(tool: ToolInfo) -> ToolSpec {
    let hints = tool.annotations.as_ref();
    ToolSpec {
        name: tool.name,
        title: tool.title,
        description: tool.description,
        input_schema: tool.input_schema,
        read_only: hints
            .and_then(|hints| hints.read_only_hint)
            .unwrap_or(false),
        open_world: hints
            .and_then(|hints| hints.open_world_hint)
            .unwrap_or(true),
    }
}

/// The result's text blocks, in order; anything else is named, not shown.
fn answer(result: CallToolResult) -> Answer {
    let parts: Vec<String> = result
        .content
        .into_iter()
        .map(|block| match block {
            ContentBlock::Text { text, .. } => text,
            other => format!("[{} content not shown]", kind(&other)),
        })
        .collect();
    let text = if parts.is_empty() {
        result
            .structured_content
            .map(|value| value.to_string())
            .unwrap_or_default()
    } else {
        parts.join("\n\n")
    };
    Answer {
        text,
        is_error: result.is_error.unwrap_or(false),
    }
}

/// `image`, `audio`, `resource`: what a block is, from its JSON tag.
fn kind(block: &ContentBlock) -> String {
    serde_json::to_value(block)
        .ok()
        .and_then(|value| value.get("type")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| "other".into())
}

#[cfg(test)]
mod tests;
