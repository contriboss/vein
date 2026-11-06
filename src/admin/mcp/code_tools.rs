//! The code context provider's tools on vein's MCP, each under its own
//! name: the follow-ups its results suggest name tools agents can call.
//! What they return is third-party text, cleaned like author text.

use rama::telemetry::tracing;
use std::sync::Arc;
use std::time::Duration;

use chrono_machines::{Backoff, ExponentialBackoff};
use mcp_host::prelude::{ExecutionContext, Server, Tool, ToolError, ToolFuture, ToolOutput};
use mcp_host::protocol::types::ToolAnnotations;
use serde_json::{Map, Value};

use crate::code_context::{Provider, ToolSpec};
use crate::untrusted;

/// The most of one answer an agent gets, in characters.
const ANSWER_LIMIT: usize = 100_000;
/// The most of a tool's description, in characters.
const DESCRIPTION_LIMIT: usize = 4_000;
/// The first wait after the provider fails to list its tools.
const FIRST_RETRY: Duration = Duration::from_secs(10);
/// The longest wait between attempts to list the provider's tools.
const MAX_RETRY: Duration = Duration::from_mins(10);

/// One of the provider's tools.
struct ProxyTool {
    spec: ToolSpec,
    description: String,
    provider: Arc<dyn Provider>,
}

impl ProxyTool {
    fn new(spec: ToolSpec, provider: Arc<dyn Provider>) -> Self {
        let theirs = spec
            .description
            .as_deref()
            .and_then(|text| untrusted::clean(text, DESCRIPTION_LIMIT))
            .unwrap_or_default();
        let leaves = if provider.remote() {
            "; the query leaves this network"
        } else {
            ""
        };
        let description = format!("From {}{leaves}. {theirs}", provider.name());
        Self {
            spec,
            description,
            provider,
        }
    }
}

impl Tool for ProxyTool {
    fn name(&self) -> &str {
        &self.spec.name
    }

    fn title(&self) -> Option<&str> {
        self.spec.title.as_deref()
    }

    fn description(&self) -> Option<&str> {
        Some(&self.description)
    }

    fn input_schema(&self) -> Value {
        self.spec.input_schema.clone()
    }

    fn annotations(&self) -> Option<ToolAnnotations> {
        Some(ToolAnnotations {
            title: None,
            read_only_hint: Some(self.spec.read_only),
            destructive_hint: Some(false),
            idempotent_hint: None,
            open_world_hint: Some(self.spec.open_world),
        })
    }

    fn execute<'a>(&'a self, ctx: ExecutionContext<'a>) -> ToolFuture<'a> {
        Box::pin(async move {
            let arguments = match ctx.params {
                Value::Object(arguments) => arguments,
                Value::Null => Map::new(),
                _ => {
                    return Err(ToolError::InvalidArguments(
                        "arguments are a JSON object".into(),
                    ));
                }
            };
            let answer = self
                .provider
                .call(&self.spec.name, arguments)
                .await
                .map_err(|err| {
                    ToolError::Execution(format!("{} failed: {err:#}", self.provider.name()))
                })?;
            let text = untrusted::clean(&answer.text, ANSWER_LIMIT).unwrap_or_default();
            if answer.is_error {
                return Err(ToolError::Execution(text));
            }
            Ok(ToolOutput::text(text))
        })
    }
}

/// Registers `provider`'s tools that `offers` lets through, once it lists
/// them: until then, retries with backoff (vein starts when the provider
/// is out of reach). Each registration tells sessions their tool list
/// changed. Must run inside a Tokio runtime.
pub fn offer(
    server: Arc<Server>,
    provider: Arc<dyn Provider>,
    offers: impl Fn(&str) -> bool + Send + 'static,
) {
    tokio::spawn(async move {
        let mut backoff = listing_backoff();
        loop {
            match provider.tools().await {
                Ok(specs) => {
                    let mut offered = 0_usize;
                    for spec in specs.into_iter().filter(|spec| offers(&spec.name)) {
                        let name = spec.name.clone();
                        match server
                            .tool_registry()
                            .register(ProxyTool::new(spec, Arc::clone(&provider)))
                        {
                            Ok(()) => offered += 1,
                            Err(err) => tracing::warn!(
                                tool = name,
                                error = %err,
                                "code context tool refused"
                            ),
                        }
                    }
                    tracing::info!(
                        provider = provider.name(),
                        offered,
                        "code context tools offered"
                    );
                    return;
                }
                Err(err) => {
                    let wait = backoff.failed().map_or(MAX_RETRY, Duration::from_millis);
                    tracing::warn!(
                        provider = provider.name(),
                        error = %format!("{err:#}"),
                        retry_in_secs = wait.as_secs(),
                        "code context tools unavailable"
                    );
                    tokio::time::sleep(wait).await;
                }
            }
        }
    });
}

/// Waits between attempts to list the provider's tools: doubling from
/// [`FIRST_RETRY`] up to [`MAX_RETRY`], never giving up, with a little jitter
/// so several vein processes started together do not retry in step.
fn listing_backoff() -> Backoff<ExponentialBackoff> {
    Backoff::new(
        ExponentialBackoff::new()
            .max_attempts(u8::MAX)
            .base_delay_ms(crate::util::millis(FIRST_RETRY))
            .max_delay_ms(crate::util::millis(MAX_RETRY))
            .jitter_factor(0.2),
    )
}

#[cfg(test)]
mod tests;
