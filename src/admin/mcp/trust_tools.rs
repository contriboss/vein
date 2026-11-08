//! The write tools: trusting a package past the release delay, and taking
//! that back. Only the admin (the write token's holder) sees or runs them.

use mcp_host::prelude::{
    ExecutionContext, Server, Tool, ToolError, ToolFuture, ToolOutput, VisibilityContext,
};
use mcp_host::protocol::types::ToolAnnotations;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use vein_adapter::AssetKind;

use super::auth;
use crate::admin::state::AdminResources;

/// The longest reason kept with a trust entry.
const REASON_LIMIT: usize = 200;

/// Registers both tools. Must run inside a Tokio runtime.
pub fn offer(server: &Server, resources: &AdminResources) -> anyhow::Result<()> {
    for trust in [true, false] {
        server
            .tool_registry()
            .register(TrustTool {
                resources: resources.clone(),
                trust,
            })
            .map_err(|err| anyhow::anyhow!("registering a trust tool: {err}"))?;
    }
    Ok(())
}

/// `trust_package` (`trust`) or `untrust_package`.
struct TrustTool {
    resources: AdminResources,
    trust: bool,
}

#[derive(Deserialize)]
struct Arguments {
    registry: String,
    name: String,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Serialize)]
struct Outcome {
    registry: &'static str,
    name: String,
    trusted: bool,
    /// Held versions trusting released; for `untrust_package`, whether the
    /// package was trusted at all.
    #[serde(skip_serializing_if = "Option::is_none")]
    released: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    was_trusted: Option<bool>,
    /// Whether `[delay_policy]` holds anything at all; trust changes nothing
    /// while it is off.
    delay_enabled: bool,
}

impl Tool for TrustTool {
    fn name(&self) -> &str {
        if self.trust {
            "trust_package"
        } else {
            "untrust_package"
        }
    }

    fn description(&self) -> Option<&str> {
        Some(if self.trust {
            "Trust a package: its versions skip the release delay, from now on and the \
             ones held right now. Exact names only, one registry at a time. Trust only \
             what you publish or vet yourself: this turns off the delay's protection \
             against a compromised release."
        } else {
            "Stop trusting a package: new versions wait out the release delay again. \
             Versions already released stay released."
        })
    }

    fn input_schema(&self) -> Value {
        let mut properties = json!({
            "registry": {
                "type": "string",
                "enum": crate::registry::served().iter().map(AssetKind::as_str).collect::<Vec<_>>(),
                "description": "The registry the package is published on."
            },
            "name": {
                "type": "string",
                "description": "The package's exact name (scoped npm names as-is, e.g. @scope/name)."
            }
        });
        if self.trust {
            properties["reason"] = json!({
                "type": "string",
                "description": "Why it is trusted, kept with the entry (e.g. \"we publish it\")."
            });
        }
        json!({"type": "object", "properties": properties, "required": ["registry", "name"]})
    }

    fn annotations(&self) -> Option<ToolAnnotations> {
        Some(ToolAnnotations {
            title: None,
            read_only_hint: Some(false),
            destructive_hint: Some(false),
            idempotent_hint: Some(true),
            open_world_hint: Some(false),
        })
    }

    fn is_visible(&self, ctx: &VisibilityContext) -> bool {
        auth::is_admin(ctx)
    }

    fn execute<'a>(&'a self, ctx: ExecutionContext<'a>) -> ToolFuture<'a> {
        Box::pin(async move {
            // Hidden is not enough: a client may call what it was not shown.
            if !auth::is_admin(&ctx) {
                return Err(ToolError::NotFound(self.name().to_owned()));
            }
            let arguments: Arguments = serde_json::from_value(ctx.params.clone())
                .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
            let kind: AssetKind = arguments
                .registry
                .parse()
                .ok()
                .filter(|kind| crate::registry::served().contains(kind))
                .ok_or_else(|| {
                    ToolError::InvalidArguments(format!(
                        "`{}` is not a registry this vein serves",
                        arguments.registry
                    ))
                })?;
            crate::registry::check_name(kind, &arguments.name)
                .map_err(|err| ToolError::InvalidArguments(format!("{err:#}")))?;
            let reason = arguments
                .reason
                .as_deref()
                .and_then(|reason| crate::untrusted::clean(reason, REASON_LIMIT));

            let failed = |err: anyhow::Error| ToolError::Execution(format!("{err:#}"));
            let mut outcome = Outcome {
                registry: kind.as_str(),
                name: arguments.name.clone(),
                trusted: self.trust,
                released: None,
                was_trusted: None,
                delay_enabled: self.resources.quarantine_enabled(),
            };
            if self.trust {
                let released = self
                    .resources
                    .trust_package(kind, &arguments.name, reason.as_deref())
                    .await
                    .map_err(failed)?;
                outcome.released = Some(released);
            } else {
                let was = self
                    .resources
                    .untrust_package(kind, &arguments.name)
                    .await
                    .map_err(failed)?;
                outcome.was_trusted = Some(was);
            }
            ToolOutput::structured(outcome).map_err(|err| ToolError::Internal(err.to_string()))
        })
    }
}
