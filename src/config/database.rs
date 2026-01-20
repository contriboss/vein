use crate::config::reliability::{ReliabilityConfig, RetryConfig};
use anyhow::{Result, bail};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_database_path")]
    pub path: PathBuf,
    #[serde(default)]
    pub url: Option<String>,
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
            if let Some(scheme) = Self::parse_scheme(trimmed)
                && scheme == "sqlite"
                && let Ok(path) = Self::sqlite_path_from_url(trimmed)
            {
                self.path = path;
            }
        }
        if self.path.is_relative() {
            self.path = base_dir.join(&self.path);
        }
    }

    pub fn ensure_directories(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    pub fn backend(&self) -> Result<DatabaseBackend> {
        if let Some(raw_url) = &self.url {
            let trimmed = raw_url.trim();
            let scheme = Self::parse_scheme(trimmed).unwrap_or_default();

            match scheme {
                "postgres" | "postgresql" => Ok(DatabaseBackend::Postgres {
                    url: trimmed.to_string(),
                }),
                "sqlite" => {
                    let parsed_path = Self::sqlite_path_from_url(trimmed)?;
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

    fn parse_scheme(url: &str) -> Option<&str> {
        url.find("://").map(|idx| &url[..idx])
    }

    fn sqlite_path_from_url(url: &str) -> Result<PathBuf> {
        // Format: sqlite://[host]/path or sqlite:///path
        let after_scheme = url
            .strip_prefix("sqlite://")
            .ok_or_else(|| anyhow::anyhow!("invalid sqlite url"))?;

        // Check for host part
        let (host, path_part) = if let Some(slash_idx) = after_scheme.find('/') {
            (&after_scheme[..slash_idx], &after_scheme[slash_idx..])
        } else {
            ("", after_scheme)
        };

        if !host.is_empty() && host != "localhost" && host != "." {
            bail!("sqlite url must not specify host (got {host})");
        }

        if path_part.is_empty() || path_part == "/" {
            bail!("sqlite url must include database path");
        }

        let path = if host == "." {
            // sqlite://./relative/path -> relative/path
            PathBuf::from(path_part.trim_start_matches('/'))
        } else {
            // sqlite:///absolute/path -> /absolute/path
            // sqlite://localhost/path -> /path
            PathBuf::from(path_part)
        };

        Ok(path)
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: default_database_path(),
            url: None,
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
    Postgres { url: String },
}
