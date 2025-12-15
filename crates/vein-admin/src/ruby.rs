use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{NaiveDate, Utc};
use reqwest::Client;
use serde::Deserialize;

const BRANCHES_URL: &str =
    "https://raw.githubusercontent.com/ruby/www.ruby-lang.org/master/_data/branches.yml";
const RELEASES_URL: &str =
    "https://raw.githubusercontent.com/ruby/www.ruby-lang.org/master/_data/releases.yml";

#[derive(Debug, Clone)]
pub struct RubyStatus {
    pub fetched_at: chrono::DateTime<Utc>,
    pub latest_release: Option<RubyRelease>,
    pub security_maintenance: Vec<BranchInfo>,
    pub recent_eol: Vec<BranchInfo>,
}

#[derive(Debug, Clone)]
pub struct RubyRelease {
    pub version: String,
    pub date: NaiveDate,
}

#[derive(Debug, Clone)]
pub struct BranchInfo {
    pub name: String,
    pub status: String,
    pub security_maintenance_date: Option<NaiveDate>,
    pub eol_date: Option<NaiveDate>,
    pub expected_eol_date: Option<NaiveDate>,
}

#[derive(Debug, Deserialize)]
struct BranchEntry {
    name: String,
    status: String,
    #[serde(rename = "date")]
    _date: Option<String>,
    #[serde(default)]
    security_maintenance_date: Option<String>,
    #[serde(default)]
    eol_date: Option<String>,
    #[serde(default)]
    expected_eol_date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReleaseEntry {
    version: String,
    date: String,
}

impl Default for RubyStatus {
    fn default() -> Self {
        Self {
            fetched_at: Utc::now(),
            latest_release: None,
            security_maintenance: Vec::new(),
            recent_eol: Vec::new(),
        }
    }
}

async fn fetch_with_retry(client: &Client, url: &str, resource_name: &str) -> Result<String> {
    const MAX_ATTEMPTS: u32 = 3;
    const INITIAL_BACKOFF_MS: u64 = 1000;

    let mut attempt = 1;

    loop {
        tracing::debug!(
            attempt = attempt,
            max_attempts = MAX_ATTEMPTS,
            resource = resource_name,
            "Fetching from GitHub"
        );

        match client.get(url).send().await {
            Ok(response) => {
                let status = response.status();

                // Check if we should retry based on status code
                if status.is_success() {
                    return response
                        .text()
                        .await
                        .context(format!("downloading {} body", resource_name));
                }

                // Retry on rate limits (429) and server errors (5xx)
                let should_retry = status.as_u16() == 429 || status.is_server_error();

                if should_retry && attempt < MAX_ATTEMPTS {
                    tracing::warn!(
                        attempt = attempt,
                        status = status.as_u16(),
                        resource = resource_name,
                        "Request failed with retryable status, retrying..."
                    );

                    let backoff_ms = INITIAL_BACKOFF_MS * 2_u64.pow(attempt - 1);
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                    attempt += 1;
                    continue;
                }

                // Don't retry on client errors (4xx except 429)
                return Err(anyhow::anyhow!(
                    "{} request failed with status {}: {}",
                    resource_name,
                    status.as_u16(),
                    status.canonical_reason().unwrap_or("Unknown")
                ));
            }
            Err(err) => {
                // Retry on network errors
                if attempt < MAX_ATTEMPTS {
                    tracing::warn!(
                        attempt = attempt,
                        error = %err,
                        resource = resource_name,
                        "Network error, retrying..."
                    );

                    let backoff_ms = INITIAL_BACKOFF_MS * 2_u64.pow(attempt - 1);
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                    attempt += 1;
                    continue;
                }

                return Err(err).context(format!(
                    "fetching {} data after {} attempts",
                    resource_name, MAX_ATTEMPTS
                ));
            }
        }
    }
}

pub async fn fetch_ruby_status() -> Result<RubyStatus> {
    let client = Client::builder()
        .user_agent("vein-admin/0.1.0")
        .timeout(Duration::from_secs(15))
        .build()
        .context("building reqwest client")?;

    let branches_text = fetch_with_retry(&client, BRANCHES_URL, "branches").await?;
    let releases_text = fetch_with_retry(&client, RELEASES_URL, "releases").await?;

    let branch_entries: Vec<BranchEntry> =
        serde_yaml::from_str(&branches_text).context("parsing branches yaml")?;
    let release_entries: Vec<ReleaseEntry> =
        serde_yaml::from_str(&releases_text).context("parsing releases yaml")?;

    let mut security = Vec::new();
    let mut eol = Vec::new();

    for entry in branch_entries {
        let info = BranchInfo {
            name: entry.name,
            status: entry.status,
            security_maintenance_date: parse_date(entry.security_maintenance_date),
            eol_date: parse_date(entry.eol_date.clone()),
            expected_eol_date: parse_date(entry.expected_eol_date),
        };
        match info.status.as_str() {
            "security maintenance" => security.push(info.clone()),
            "eol" => eol.push(info.clone()),
            _ => {}
        }
    }

    security.sort_by_key(|b| b.expected_eol_date.unwrap_or(NaiveDate::MAX));
    eol.sort_by_key(|b| b.eol_date.unwrap_or(NaiveDate::MIN));
    eol.reverse();
    let recent_eol = eol.into_iter().take(3).collect();

    let latest_release = release_entries.into_iter().find_map(|release| {
        parse_date(Some(release.date)).map(|date| RubyRelease {
            version: release.version,
            date,
        })
    });

    Ok(RubyStatus {
        fetched_at: Utc::now(),
        latest_release,
        security_maintenance: security,
        recent_eol,
    })
}

fn parse_date(raw: Option<String>) -> Option<NaiveDate> {
    raw.and_then(|value| NaiveDate::parse_from_str(&value, "%Y-%m-%d").ok())
}
