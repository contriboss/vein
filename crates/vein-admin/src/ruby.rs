use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use chrono::{NaiveDate, Utc};
use rama::{
    Service,
    error::{ErrorExt as _, extra::OpaqueError},
    http::{
        Body, Request, Response, StatusCode, body::util::BodyExt, client::EasyHttpWebClient,
        header::HeaderValue, layer::required_header::AddRequiredRequestHeadersLayer,
    },
    layer::{Layer, MapErrLayer, TimeoutLayer},
};
use serde::{Deserialize, de::DeserializeOwned};

const BRANCHES_URL: &str =
    "https://raw.githubusercontent.com/ruby/www.ruby-lang.org/master/_data/branches.yml";
const RELEASES_URL: &str =
    "https://raw.githubusercontent.com/ruby/www.ruby-lang.org/master/_data/releases.yml";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_FETCH_ATTEMPTS: u32 = 3;
const RECENT_EOL_LIMIT: usize = 3;
const USER_AGENT: &str = "vein-admin/0.1.0";

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

#[derive(Debug, Clone, Copy)]
enum RubyResource {
    Branches,
    Releases,
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

impl RubyStatus {
    fn from_sources(branches: Vec<BranchEntry>, releases: Vec<ReleaseEntry>) -> Self {
        let (security_maintenance, recent_eol) = partition_branches(branches);

        Self {
            fetched_at: Utc::now(),
            latest_release: latest_release(releases),
            security_maintenance,
            recent_eol,
        }
    }
}

impl RubyResource {
    fn label(self) -> &'static str {
        match self {
            Self::Branches => "branches",
            Self::Releases => "releases",
        }
    }

    fn url(self) -> &'static str {
        match self {
            Self::Branches => BRANCHES_URL,
            Self::Releases => RELEASES_URL,
        }
    }
}

/// Build rama HTTP client with timeout and user-agent
fn build_client() -> Result<impl Service<Request, Output = Response, Error = OpaqueError>> {
    let inner = EasyHttpWebClient::default();

    Ok((
        MapErrLayer::new(|e: rama::error::BoxError| e.into_opaque_error()),
        TimeoutLayer::new(REQUEST_TIMEOUT),
        AddRequiredRequestHeadersLayer::new()
            .with_user_agent_header_value(HeaderValue::from_static(USER_AGENT)),
    )
        .into_layer(inner))
}

async fn fetch_yaml<T>(
    client: &impl Service<Request, Output = Response, Error = OpaqueError>,
    resource: RubyResource,
) -> Result<T>
where
    T: DeserializeOwned,
{
    let body = fetch_with_retry(client, resource).await?;
    serde_yaml::from_str(&body).with_context(|| format!("parsing {} yaml", resource.label()))
}

async fn fetch_with_retry(
    client: &impl Service<Request, Output = Response, Error = OpaqueError>,
    resource: RubyResource,
) -> Result<String> {
    let mut attempt = 1;

    loop {
        tracing::debug!(
            attempt = attempt,
            max_attempts = MAX_FETCH_ATTEMPTS,
            resource = resource.label(),
            "Fetching from GitHub"
        );

        let request = build_request(resource)?;

        match client.serve(request).await {
            Ok(response) => {
                let status = response.status();

                if status.is_success() {
                    let body_bytes = response
                        .into_body()
                        .collect()
                        .await
                        .with_context(|| format!("collecting {} body", resource.label()))?
                        .to_bytes();

                    return String::from_utf8(body_bytes.to_vec())
                        .with_context(|| format!("decoding {} as UTF-8", resource.label()));
                }

                if should_retry_status(status) && attempt < MAX_FETCH_ATTEMPTS {
                    tracing::warn!(
                        attempt = attempt,
                        status = status.as_u16(),
                        resource = resource.label(),
                        "Request failed with retryable status, retrying..."
                    );

                    tokio::time::sleep(backoff_for_attempt(attempt)).await;
                    attempt += 1;
                    continue;
                }

                return Err(anyhow!(
                    "{} request failed with status {}: {}",
                    resource.label(),
                    status.as_u16(),
                    status.canonical_reason().unwrap_or("Unknown")
                ));
            }
            Err(err) => {
                if attempt < MAX_FETCH_ATTEMPTS {
                    tracing::warn!(
                        attempt = attempt,
                        error = ?err,
                        resource = resource.label(),
                        "Network error, retrying..."
                    );

                    tokio::time::sleep(backoff_for_attempt(attempt)).await;
                    attempt += 1;
                    continue;
                }

                return Err(anyhow!(
                    "fetching {} data after {} attempts: {}",
                    resource.label(),
                    MAX_FETCH_ATTEMPTS,
                    err
                ));
            }
        }
    }
}

pub async fn fetch_ruby_status() -> Result<RubyStatus> {
    let client = build_client().context("building HTTP client")?;
    let branches = fetch_yaml(&client, RubyResource::Branches).await?;
    let releases = fetch_yaml(&client, RubyResource::Releases).await?;
    Ok(RubyStatus::from_sources(branches, releases))
}

fn parse_date(raw: Option<String>) -> Option<NaiveDate> {
    raw.and_then(|value| NaiveDate::parse_from_str(&value, "%Y-%m-%d").ok())
}

fn build_request(resource: RubyResource) -> Result<Request<Body>> {
    Request::builder()
        .method("GET")
        .uri(resource.url())
        .body(Body::empty())
        .context("building request")
}

fn should_retry_status(status: StatusCode) -> bool {
    status.as_u16() == 429 || status.is_server_error()
}

fn backoff_for_attempt(attempt: u32) -> Duration {
    INITIAL_BACKOFF * 2_u32.pow(attempt.saturating_sub(1))
}

fn partition_branches(branches: Vec<BranchEntry>) -> (Vec<BranchInfo>, Vec<BranchInfo>) {
    let mut security = Vec::new();
    let mut eol = Vec::new();

    for branch in branches.into_iter().map(BranchInfo::from) {
        match branch.status.as_str() {
            "security maintenance" => security.push(branch),
            "eol" => eol.push(branch),
            _ => {}
        }
    }

    security.sort_by_key(|branch| branch.expected_eol_date.unwrap_or(NaiveDate::MAX));
    eol.sort_by_key(|branch| branch.eol_date.unwrap_or(NaiveDate::MIN));
    eol.reverse();
    eol.truncate(RECENT_EOL_LIMIT);

    (security, eol)
}

fn latest_release(releases: Vec<ReleaseEntry>) -> Option<RubyRelease> {
    releases.into_iter().find_map(RubyRelease::from_entry)
}

impl From<BranchEntry> for BranchInfo {
    fn from(entry: BranchEntry) -> Self {
        Self {
            name: entry.name,
            status: entry.status,
            security_maintenance_date: parse_date(entry.security_maintenance_date),
            eol_date: parse_date(entry.eol_date),
            expected_eol_date: parse_date(entry.expected_eol_date),
        }
    }
}

impl RubyRelease {
    fn from_entry(entry: ReleaseEntry) -> Option<Self> {
        parse_date(Some(entry.date)).map(|date| Self {
            version: entry.version,
            date,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn branch_entry(
        name: &str,
        status: &str,
        security_maintenance_date: Option<&str>,
        eol_date: Option<&str>,
        expected_eol_date: Option<&str>,
    ) -> BranchEntry {
        BranchEntry {
            name: name.to_string(),
            status: status.to_string(),
            _date: None,
            security_maintenance_date: security_maintenance_date.map(str::to_string),
            eol_date: eol_date.map(str::to_string),
            expected_eol_date: expected_eol_date.map(str::to_string),
        }
    }

    fn release_entry(version: &str, date: &str) -> ReleaseEntry {
        ReleaseEntry {
            version: version.to_string(),
            date: date.to_string(),
        }
    }

    #[test]
    fn partition_branches_sorts_security_and_limits_recent_eol() {
        let (security, recent_eol) = partition_branches(vec![
            branch_entry(
                "3.1",
                "security maintenance",
                Some("2026-04-01"),
                None,
                Some("2026-05-01"),
            ),
            branch_entry(
                "3.0",
                "security maintenance",
                Some("2026-03-01"),
                None,
                Some("2026-04-01"),
            ),
            branch_entry("2.7", "eol", None, Some("2025-01-01"), None),
            branch_entry("2.6", "eol", None, Some("2024-01-01"), None),
            branch_entry("2.5", "eol", None, Some("2023-01-01"), None),
            branch_entry("2.4", "eol", None, Some("2022-01-01"), None),
            branch_entry("3.2", "normal maintenance", None, None, None),
        ]);

        assert_eq!(
            security
                .iter()
                .map(|branch| branch.name.as_str())
                .collect::<Vec<_>>(),
            vec!["3.0", "3.1"]
        );
        assert_eq!(
            recent_eol
                .iter()
                .map(|branch| branch.name.as_str())
                .collect::<Vec<_>>(),
            vec!["2.7", "2.6", "2.5"]
        );
    }

    #[test]
    fn latest_release_returns_first_parseable_release() {
        let latest = latest_release(vec![
            release_entry("3.4.1", "invalid"),
            release_entry("3.4.0", "2026-01-15"),
            release_entry("3.3.9", "2025-12-25"),
        ])
        .unwrap();

        assert_eq!(latest.version, "3.4.0");
        assert_eq!(latest.date, NaiveDate::from_ymd_opt(2026, 1, 15).unwrap());
    }

    #[test]
    fn ruby_status_from_sources_builds_expected_snapshot() {
        let status = RubyStatus::from_sources(
            vec![
                branch_entry(
                    "3.0",
                    "security maintenance",
                    Some("2026-03-01"),
                    None,
                    Some("2026-04-01"),
                ),
                branch_entry("2.7", "eol", None, Some("2025-01-01"), None),
            ],
            vec![release_entry("3.4.0", "2026-01-15")],
        );

        assert_eq!(
            status
                .latest_release
                .as_ref()
                .map(|release| release.version.as_str()),
            Some("3.4.0")
        );
        assert_eq!(status.security_maintenance.len(), 1);
        assert_eq!(status.recent_eol.len(), 1);
    }

    #[test]
    fn backoff_for_attempt_doubles_each_retry() {
        assert_eq!(backoff_for_attempt(1), Duration::from_secs(1));
        assert_eq!(backoff_for_attempt(2), Duration::from_secs(2));
        assert_eq!(backoff_for_attempt(3), Duration::from_secs(4));
    }

    #[test]
    fn should_retry_status_only_retries_retryable_responses() {
        assert!(should_retry_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(should_retry_status(StatusCode::BAD_GATEWAY));
        assert!(!should_retry_status(StatusCode::NOT_FOUND));
    }
}
