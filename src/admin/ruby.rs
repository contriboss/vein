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
const USER_AGENT: &str = crate::upstream::UA;

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
    const fn label(self) -> &'static str {
        match self {
            Self::Branches => "branches",
            Self::Releases => "releases",
        }
    }

    const fn url(self) -> &'static str {
        match self {
            Self::Branches => BRANCHES_URL,
            Self::Releases => RELEASES_URL,
        }
    }
}

/// Build rama HTTP client with timeout and user-agent
fn build_client() -> impl Service<Request, Output = Response, Error = OpaqueError> {
    let inner = EasyHttpWebClient::default();

    (
        MapErrLayer::new(|e: rama::error::BoxError| e.into_opaque_error()),
        TimeoutLayer::new(REQUEST_TIMEOUT),
        AddRequiredRequestHeadersLayer::new()
            .with_user_agent_header_value(HeaderValue::from_static(USER_AGENT)),
    )
        .into_layer(inner)
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
    let client = build_client();
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
mod tests;
