use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceRouter;
use serde::Serialize;

use crate::admin::mcp::{VeinMcp, json_resource, read_error};

const URI: &str = "vein://trusted";

#[derive(Serialize)]
struct Trusted {
    /// Whether `[delay_policy]` holds anything; trust matters only while on.
    delay_enabled: bool,
    packages: Vec<Entry>,
}

#[derive(Serialize)]
struct Entry {
    registry: &'static str,
    name: String,
    /// Why, as the admin put it.
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    trusted_at: String,
}

impl VeinMcp {
    /// Packages trusted past the release delay at runtime (the
    /// `trust_package` tool), by registry and name. Packages the config
    /// exempts (`[[delay_policy.packages]]` with `delay_days = 0`) are not
    /// listed here.
    #[mcp_resource(
        uri = "vein://trusted",
        name = "trusted",
        mime_type = "application/json"
    )]
    async fn trusted(&self, _ctx: Ctx<'_>) -> ResourceResult {
        let packages = self
            .resources
            .trusted_packages()
            .await
            .map_err(read_error)?
            .into_iter()
            .map(|package| Entry {
                registry: package.kind.as_str(),
                name: package.name,
                reason: package.reason,
                trusted_at: package.trusted_at.to_string(),
            })
            .collect();
        json_resource(
            URI,
            &Trusted {
                delay_enabled: self.resources.quarantine_enabled(),
                packages,
            },
        )
    }
}

pub fn mount(router: McpResourceRouter<VeinMcp>) -> Result<McpResourceRouter<VeinMcp>, AppsError> {
    Ok(router.with_resource(
        VeinMcp::trusted_resource_info()?,
        VeinMcp::trusted_handler,
        None,
    ))
}
