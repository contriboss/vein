//! Argument completion for the package templates: names from the gem
//! catalog or the cache, versions from the cache.

use rama::telemetry::tracing;
use std::sync::Arc;

use mcp_host::managers::completion::{CompletionFuture, CompletionProvider};
use mcp_host::prelude::{CompletionValue, Server};
use mcp_host::protocol::types::CompletionContext;
use vein_adapter::AssetKind;

use super::templates;
use crate::admin::state::AdminResources;

const LIMIT: i64 = 100;

struct PackageArguments {
    resources: AdminResources,
    kind: AssetKind,
}

pub fn register(server: &Server, resources: &AdminResources) {
    for kind in crate::registry::served() {
        let provider: Arc<dyn CompletionProvider> = Arc::new(PackageArguments {
            resources: resources.clone(),
            kind,
        });
        for template in templates::of(kind) {
            server
                .completion_manager()
                .register_resource_provider(*template, Arc::clone(&provider));
        }
    }
}

impl PackageArguments {
    async fn names(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
        match self.kind {
            // The gem catalog covers all of rubygems.org, not just the cache.
            AssetKind::Gem | AssetKind::Spec => self.resources.catalog_search(prefix, LIMIT).await,
            kind => self.resources.search_cached(kind, prefix, LIMIT).await,
        }
    }

    async fn versions(&self, name: &str, prefix: &str) -> anyhow::Result<Vec<String>> {
        let mut versions = self.resources.version_names(self.kind, name).await?;
        versions.retain(|version| version.starts_with(prefix));
        Ok(versions)
    }
}

impl CompletionProvider for PackageArguments {
    fn complete_resource<'a>(
        &'a self,
        _uri: &'a str,
        argument_name: &'a str,
        argument_value: &'a str,
        context: Option<&'a CompletionContext>,
    ) -> CompletionFuture<'a> {
        Box::pin(async move {
            let name = context
                .and_then(|context| context.arguments.as_ref())
                .and_then(|arguments| arguments.get("name"));
            let values = match (argument_name, name) {
                ("name" | "query", _) if !argument_value.is_empty() => {
                    self.names(argument_value).await
                }
                ("version", Some(name)) => self.versions(name, argument_value).await,
                _ => Ok(Vec::new()),
            };
            values
                .unwrap_or_else(|err| {
                    tracing::warn!(error = %err, argument_name, "MCP completion failed");
                    Vec::new()
                })
                .into_iter()
                .map(CompletionValue::new)
                .collect()
        })
    }
}
