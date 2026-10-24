//! Where the MCP's code context tools come from: a provider indexing public
//! open-source code and docs beyond this cache. Off unless one is named;
//! its queries leave this network.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// The environment variable holding the `GitHits` API token (`ghi-...`).
pub const GITHITS_TOKEN_ENV: &str = "VEIN_GITHITS_TOKEN";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CodeContextConfig {
    /// The provider of the MCP's code tools; none when left out.
    pub provider: Option<Provider>,
    /// Only these of the provider's tools; all of them when left out.
    pub tools: Option<Vec<String>>,
    pub githits: GitHitsConfig,
}

impl CodeContextConfig {
    /// Whether the provider's tool `name` is offered: none without one.
    #[must_use]
    pub fn offers(&self, name: &str) -> bool {
        self.provider.is_some()
            && self
                .tools
                .as_ref()
                .is_none_or(|tools| tools.iter().any(|tool| tool == name))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    /// <https://githits.com>, over its hosted MCP.
    GitHits,
}

impl Provider {
    /// The Cargo feature that builds the provider in.
    #[must_use]
    pub const fn feature(self) -> &'static str {
        match self {
            Self::GitHits => "githits",
        }
    }

    #[must_use]
    pub const fn built(self) -> bool {
        match self {
            Self::GitHits => cfg!(feature = "githits"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GitHitsConfig {
    /// The hosted MCP endpoint.
    pub url: String,
    /// A file holding the API token, when [`GITHITS_TOKEN_ENV`] is not set:
    /// kept out of `vein.toml`, readable by vein alone. `~/` is the home of
    /// the user vein runs as.
    pub token_file: Option<PathBuf>,
}

impl Default for GitHitsConfig {
    fn default() -> Self {
        Self {
            url: "https://mcp.githits.com".into(),
            token_file: None,
        }
    }
}

impl GitHitsConfig {
    /// The API token: [`GITHITS_TOKEN_ENV`], else the first line of
    /// `token_file`.
    pub fn token(&self) -> Result<String> {
        if let Some(token) = std::env::var(GITHITS_TOKEN_ENV)
            .ok()
            .filter(|token| !token.trim().is_empty())
        {
            return Ok(token.trim().to_owned());
        }
        let Some(file) = &self.token_file else {
            bail!(
                "code_context.provider is githits, but neither {GITHITS_TOKEN_ENV} nor \
                 code_context.githits.token_file gives its API token"
            );
        };
        let file = home_relative(file);
        let text = std::fs::read_to_string(&file)
            .with_context(|| format!("reading the GitHits token from {}", file.display()))?;
        let token = text.lines().next().unwrap_or_default().trim();
        if token.is_empty() {
            bail!("{} holds no GitHits token", file.display());
        }
        Ok(token.to_owned())
    }
}

/// `path`, with a leading `~/` as `$HOME/`.
fn home_relative(path: &std::path::Path) -> PathBuf {
    match (path.strip_prefix("~"), std::env::var_os("HOME")) {
        (Ok(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests;
