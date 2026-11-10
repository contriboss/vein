//! `audit_lockfile`: a project's lockfile against the advisories vein
//! holds. Only the vulnerable dependencies come back, each with the version
//! that fixes every advisory against it; a clean lockfile is one line.

use mcp_host::prelude::{ExecutionContext, Server, Tool, ToolError, ToolFuture, ToolOutput};
use mcp_host::protocol::types::ToolAnnotations;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::advisories::{Note, for_version};
use crate::admin::state::AdminResources;
use crate::advisories::{compare, lockfile};

/// The largest lockfile read: a big monorepo's `package-lock.json` is a
/// few MB.
const LOCKFILE_LIMIT: usize = 16 * 1024 * 1024;
/// Above this many advisories against one dependency, summaries (and the
/// ranges of those naming a fix) are left out (a years-old rack pin has 30-odd): the ids, severities and fix are
/// what an upgrade needs, each version's resource has the rest.
const SUMMARIES_UP_TO: usize = 5;

/// Registers the tool. Must run inside a Tokio runtime.
pub fn offer(server: &Server, resources: &AdminResources) -> anyhow::Result<()> {
    server
        .tool_registry()
        .register(AuditTool {
            resources: resources.clone(),
        })
        .map_err(|err| anyhow::anyhow!("registering audit_lockfile: {err}"))
}

struct AuditTool {
    resources: AdminResources,
}

#[derive(Deserialize)]
struct Arguments {
    lockfile: String,
}

#[derive(Serialize)]
struct Audit {
    registry: &'static str,
    /// Registry packages the lockfile pins.
    checked: usize,
    vulnerable: Vec<Vulnerable>,
}

#[derive(Serialize)]
struct Vulnerable {
    name: String,
    version: String,
    /// The lowest version out of every advisory below, when each names a
    /// fix.
    #[serde(skip_serializing_if = "Option::is_none")]
    upgrade_to: Option<String>,
    advisories: Vec<Note>,
}

impl Tool for AuditTool {
    fn name(&self) -> &'static str {
        "audit_lockfile"
    }

    fn description(&self) -> Option<&str> {
        Some(
            "Check a lockfile against known security advisories (GitHub's reviewed \
             database): Cargo.lock, Gemfile.lock or package-lock.json, passed as its \
             contents. Returns only the dependencies pinned at a vulnerable version, \
             each with its advisories and the version to upgrade to.",
        )
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "lockfile": {
                    "type": "string",
                    "description": "The lockfile's full contents."
                }
            },
            "required": ["lockfile"]
        })
    }

    fn annotations(&self) -> Option<ToolAnnotations> {
        Some(ToolAnnotations {
            title: None,
            read_only_hint: Some(true),
            destructive_hint: Some(false),
            idempotent_hint: Some(true),
            open_world_hint: Some(false),
        })
    }

    fn execute<'a>(&'a self, ctx: ExecutionContext<'a>) -> ToolFuture<'a> {
        Box::pin(async move {
            let arguments: Arguments = serde_json::from_value(ctx.params.clone())
                .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
            if arguments.lockfile.len() > LOCKFILE_LIMIT {
                return Err(ToolError::InvalidArguments(format!(
                    "lockfile over {} MB",
                    LOCKFILE_LIMIT / 1024 / 1024
                )));
            }
            let (kind, pinned) = lockfile::read(&arguments.lockfile)
                .map_err(|err| ToolError::InvalidArguments(format!("{err:#}")))?;
            let names: Vec<String> = pinned.iter().map(|p| p.name.clone()).collect();
            let advisories = self
                .resources
                .advisories(kind, &names)
                .await
                .map_err(|err| ToolError::Execution(format!("{err:#}")))?;

            let vulnerable = pinned
                .iter()
                .filter_map(|package| {
                    let about: Vec<_> = advisories
                        .iter()
                        .filter(|advisory| advisory.packages.iter().any(|p| p.name == package.name))
                        .map(|advisory| vein_adapter::Advisory {
                            packages: advisory
                                .packages
                                .iter()
                                .filter(|p| p.name == package.name)
                                .cloned()
                                .collect(),
                            ..advisory.clone()
                        })
                        .collect();
                    let mut notes = for_version(kind, &about, &package.version);
                    shorten(&mut notes);
                    (!notes.is_empty()).then(|| Vulnerable {
                        upgrade_to: upgrade_to(kind, &notes),
                        name: package.name.clone(),
                        version: package.version.clone(),
                        advisories: notes,
                    })
                })
                .collect();
            ToolOutput::structured(Audit {
                registry: kind.as_str(),
                checked: pinned.len(),
                vulnerable,
            })
            .map_err(|err| ToolError::Internal(err.to_string()))
        })
    }
}

/// Past [`SUMMARIES_UP_TO`] advisories, leaves summaries out, and the
/// vulnerable ranges of those whose `fixed_in` already says what to do.
fn shorten(notes: &mut [Note]) {
    if notes.len() > SUMMARIES_UP_TO {
        for note in notes {
            note.summary = None;
            if note.fixed_in.is_some() {
                note.vulnerable.clear();
            }
        }
    }
}

/// The highest of the fixes, when every advisory names one.
fn upgrade_to(kind: vein_adapter::AssetKind, notes: &[Note]) -> Option<String> {
    notes
        .iter()
        .map(|note| note.fixed_in.as_deref())
        .collect::<Option<Vec<&str>>>()?
        .into_iter()
        .max_by(|a, b| compare(kind, a, b))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests;
