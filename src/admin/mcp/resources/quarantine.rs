use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceRouter;
use serde::Serialize;

use crate::admin::mcp::views::{QuarantineCounts, TrackedVersion};
use crate::admin::mcp::{VeinMcp, json_resource, read_error};

const URI: &str = "vein://quarantine";
const PENDING_LIMIT: u32 = 200;

#[derive(Serialize)]
struct Quarantine {
    /// Whether the release delay policy is on. When off, nothing is held back.
    enabled: bool,
    counts: QuarantineCounts,
    /// Versions still held back, soonest release first.
    pending: Vec<Pending>,
}

#[derive(Serialize)]
struct Pending {
    /// `gem`, `crate` or `npm`.
    registry: &'static str,
    #[serde(flatten)]
    version: TrackedVersion,
}

impl VeinMcp {
    /// New gem, crate and npm versions held back by the release delay
    /// policy, and when each becomes installable through this proxy.
    #[mcp_resource(
        uri = "vein://quarantine",
        name = "quarantine",
        mime_type = "application/json"
    )]
    async fn quarantine(&self, _ctx: Ctx<'_>) -> ResourceResult {
        let pending = self
            .resources
            .quarantine_pending(PENDING_LIMIT, 0)
            .await
            .map_err(read_error)?;
        let counts = self
            .resources
            .quarantine_stats()
            .await
            .map_err(read_error)?;
        json_resource(
            URI,
            &Quarantine {
                enabled: self.resources.quarantine_enabled(),
                counts: counts.into(),
                pending: pending
                    .into_iter()
                    .map(|version| Pending {
                        registry: version.kind.as_str(),
                        version: version.into(),
                    })
                    .collect(),
            },
        )
    }
}

pub fn mount(router: McpResourceRouter<VeinMcp>) -> Result<McpResourceRouter<VeinMcp>, AppsError> {
    Ok(router.with_resource(
        VeinMcp::quarantine_resource_info()?,
        VeinMcp::quarantine_handler,
        None,
    ))
}
