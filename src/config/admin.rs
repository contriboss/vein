use serde::Deserialize;

/// The admin dashboard and its `/mcp` endpoint, served on their own listener
/// next to the proxy. Only built with the `admin` feature.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AdminConfig {
    /// Serve the dashboard. Builds without the `admin` feature warn and skip it.
    pub enabled: bool,
    /// Bind host. Anything but loopback needs `VEIN_ADMIN_MCP_TOKEN` for MCP.
    pub host: String,
    pub port: u16,
    pub mcp: McpSettings,
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            host: "127.0.0.1".to_string(),
            port: 9400,
            mcp: McpSettings::default(),
        }
    }
}

impl AdminConfig {
    #[must_use]
    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// The `/mcp` endpoint. Its bearer token is read from the environment
/// (`VEIN_ADMIN_MCP_TOKEN`), never from this file.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct McpSettings {
    pub enabled: bool,
}

impl Default for McpSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}
