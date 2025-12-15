use crate::config::reliability::{ReliabilityConfig, RetryConfig};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_database_path")]
    pub path: PathBuf,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default = "DatabaseConfig::default_max_connections")]
    pub max_connections: u32,
    #[serde(default = "DatabaseConfig::default_reliability")]
    pub reliability: ReliabilityConfig,
}

impl DatabaseConfig {
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

    pub fn normalize_paths(&mut self, base_dir: &Path) {
        if let Some(raw_url) = &self.url {
            let trimmed = raw_url.trim();
            if let Ok(parsed) = url::Url::parse(trimmed)
                && parsed.scheme() == "sqlite"
                && let Ok(path) = Self::sqlite_path_from_url(&parsed)
            {
                self.path = path;
            }
        }
        if self.path.is_relative() {
            self.path = base_dir.join(&self.path);
        }
    }

    fn default_max_connections() -> u32 {
        16
    }

    pub fn backend(&self) -> Result<DatabaseBackend> {
        if let Some(raw_url) = &self.url {
            let trimmed = raw_url.trim();
            let parsed = url::Url::parse(trimmed)
                .with_context(|| format!("parsing database url '{trimmed}'"))?;

            match parsed.scheme() {
                "postgres" | "postgresql" => Ok(DatabaseBackend::Postgres {
                    url: trimmed.to_string(),
                    max_connections: self.max_connections.max(1),
                }),
                "sqlite" => {
                    let parsed_path = Self::sqlite_path_from_url(&parsed)?;
                    let path = if self.path.is_absolute() {
                        self.path.clone()
                    } else {
                        parsed_path
                    };
                    Ok(DatabaseBackend::Sqlite { path })
                }
                other => bail!("unsupported database url scheme {other}"),
            }
        } else {
            Ok(DatabaseBackend::Sqlite {
                path: self.path.clone(),
            })
        }
    }

    fn sqlite_path_from_url(url: &url::Url) -> Result<PathBuf> {
        let host_opt = url.host_str();
        if let Some(host) = host_opt
            && !host.is_empty()
            && host != "localhost"
            && host != "."
        {
            bail!("sqlite url must not specify host (got {host})");
        }
        let path_str = url.path();
        if path_str.is_empty() || path_str == "/" {
            bail!("sqlite url must include database path");
        }

        let path = if matches!(host_opt, Some(host) if host == ".") {
            PathBuf::from(path_str.trim_start_matches('/'))
        } else if path_str.starts_with('/') {
            PathBuf::from(path_str)
        } else {
            PathBuf::from(path_str.trim_start_matches('/'))
        };

        Ok(path)
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: default_database_path(),
            url: None,
            max_connections: Self::default_max_connections(),
            reliability: Self::default_reliability(),
        }
    }
}

fn default_database_path() -> PathBuf {
    PathBuf::from("./vein.db")
}

#[derive(Debug, Clone)]
pub enum DatabaseBackend {
    Sqlite { path: PathBuf },
    Postgres { url: String, max_connections: u32 },
}
