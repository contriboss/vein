use crate::config::reliability::{ReliabilityConfig, RetryConfig};
use anyhow::{Result, bail};
use serde::Deserialize;

/// The Postgres cache index.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DatabaseConfig {
    /// `postgres://user:pass@host/db`; required to run.
    pub url: Option<String>,
    /// Connection pool size.
    pub max_connections: u32,
    pub reliability: ReliabilityConfig,
}

impl DatabaseConfig {
    const DEFAULT_MAX_CONNECTIONS: u32 = 10;

    fn default_reliability() -> ReliabilityConfig {
        ReliabilityConfig {
            retry: RetryConfig {
                max_attempts: 5,
                initial_backoff_ms: 500,
                max_backoff_secs: 30,
                ..RetryConfig::default()
            },
        }
    }

    /// Where the database lives, for display: the URL with any credentials
    /// removed.
    #[must_use]
    pub fn location(&self) -> String {
        self.url
            .as_deref()
            .map_or_else(String::new, redact_credentials)
    }

    /// The validated connection target.
    ///
    /// # Errors
    ///
    /// When `url` is unset or is not a `postgres://` URL.
    pub fn backend(&self) -> Result<DatabaseBackend> {
        let Some(raw_url) = &self.url else {
            bail!("database.url is required, e.g. postgres://user:pass@host/vein");
        };
        let url = raw_url.trim();
        match url.split_once("://").map(|(scheme, _)| scheme) {
            Some("postgres" | "postgresql") => Ok(DatabaseBackend {
                url: url.to_string(),
            }),
            Some(other) => bail!("unsupported database url scheme {other}; vein needs postgres"),
            None => bail!("database.url {url:?} is not a URL"),
        }
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: None,
            max_connections: Self::DEFAULT_MAX_CONNECTIONS,
            reliability: Self::default_reliability(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DatabaseBackend {
    pub url: String,
}

/// `url` without its `user:password@` part, so it can be shown on a dashboard.
pub(crate) fn redact_credentials(url: &str) -> String {
    if let Some((scheme, rest)) = url.split_once("://")
        && let Some((_, host)) = rest.split_once('@')
    {
        return format!("{scheme}://{host}");
    }
    url.to_owned()
}
