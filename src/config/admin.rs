use std::path::PathBuf;

use anyhow::Result;
use serde::Deserialize;

/// The admin dashboard and its `/mcp` endpoint, served on their own listener
/// next to the proxy. Only built with the `admin` feature.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AdminConfig {
    /// Serve the dashboard. Builds without the `admin` feature warn and skip it.
    pub enabled: bool,
    /// Bind host. Anything but loopback needs `trusted_networks` or
    /// `VEIN_ADMIN_MCP_TOKEN` for MCP.
    pub host: String,
    pub port: u16,
    /// Networks (CIDR blocks or addresses) admitted to the whole admin
    /// listener, dashboard and `/mcp` alike; everyone else gets `403`.
    /// Loopback is always added. They stand in for the MCP token on a
    /// non-loopback bind; with both set, a request needs both.
    pub trusted_networks: Vec<String>,
    pub mcp: McpSettings,
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            host: "127.0.0.1".to_string(),
            port: 9400,
            trusted_networks: Vec::new(),
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

/// The `/mcp` endpoint. Its bearer tokens are read from the environment
/// (`VEIN_ADMIN_MCP_TOKEN`, `VEIN_ADMIN_MCP_WRITE_TOKEN`), never from this
/// file.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct McpSettings {
    pub enabled: bool,
    /// Offer the tools that change the cache's policy (trusting packages
    /// past the release delay). Off by default; on, they still need
    /// `VEIN_ADMIN_MCP_WRITE_TOKEN` (or `write_token_file`), sent as a bearer
    /// token on every call.
    pub writes: bool,
    /// A file holding the write token, when `VEIN_ADMIN_MCP_WRITE_TOKEN` is
    /// not set: kept out of `vein.toml`, readable by vein alone.
    pub write_token_file: Option<PathBuf>,
}

impl Default for McpSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            writes: false,
            write_token_file: None,
        }
    }
}

impl McpSettings {
    /// The write token: `env` (`VEIN_ADMIN_MCP_WRITE_TOKEN`) when set, else
    /// the first line of `write_token_file`; `None` without either.
    pub fn write_token(&self, env: Option<String>) -> Result<Option<String>> {
        if let Some(token) = env
            .map(|token| token.trim().to_owned())
            .filter(|t| !t.is_empty())
        {
            return Ok(Some(token));
        }
        self.write_token_file
            .as_deref()
            .map(|file| super::read_token_file(file, "MCP write"))
            .transpose()
    }
}

#[cfg(test)]
mod tests;
