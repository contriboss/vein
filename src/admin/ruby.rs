use rama::telemetry::tracing;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use chrono_machines::{AsyncRetryable, ExponentialBackoff};
use jiff::Timestamp;
use jiff::civil::Date;
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
const MAX_FETCH_ATTEMPTS: u8 = 3;
const RECENT_EOL_LIMIT: usize = 3;
const USER_AGENT: &str = crate::upstream::UA;

#[derive(Debug, Clone)]
pub struct RubyStatus {
    pub fetched_at: Timestamp,
    pub latest_release: Option<RubyRelease>,
    pub security_maintenance: Vec<BranchInfo>,
    pub recent_eol: Vec<BranchInfo>,
}

#[derive(Debug, Clone)]
pub struct RubyRelease {
    pub version: String,
    pub date: Date,
}

#[derive(Debug, Clone)]
pub struct BranchInfo {
    pub name: String,
    pub status: String,
    pub security_maintenance_date: Option<Date>,
    pub eol_date: Option<Date>,
    pub expected_eol_date: Option<Date>,
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
            fetched_at: Timestamp::now(),
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
            fetched_at: Timestamp::now(),
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
    let result = (|| fetch_once(client, resource))
        .retry_async(fetch_backoff())
        .when(|failure: &FetchFailure| failure.retryable)
        .notify(move |ctx| {
            tracing::warn!(
                attempt = ctx.attempt,
                max_attempts = MAX_FETCH_ATTEMPTS,
                retry_in_ms = ctx.next_delay_ms,
                resource = resource.label(),
                error = ctx
                    .error
                    .map(|failure| tracing::field::display(&failure.error)),
                "Fetching from GitHub failed, retrying"
            );
        })
        .call_async(crate::util::sleep_ms)
        .await;

    match result {
        Ok(outcome) => Ok(outcome.into_inner()),
        Err(err) => {
            let attempts = err.attempts();
            Err(err.into_cause().error.context(format!(
                "fetching {} data ({attempts} attempts)",
                resource.label()
            )))
        }
    }
}

/// One failed fetch, and whether trying again may help.
struct FetchFailure {
    retryable: bool,
    error: anyhow::Error,
}

impl FetchFailure {
    const fn retryable(error: anyhow::Error) -> Self {
        Self {
            retryable: true,
            error,
        }
    }

    const fn fatal(error: anyhow::Error) -> Self {
        Self {
            retryable: false,
            error,
        }
    }
}

/// One GET of `resource`: network failures, a body cut short, 429 and 5xx
/// are worth retrying; any other status or a body that is not UTF-8 is not.
async fn fetch_once(
    client: &impl Service<Request, Output = Response, Error = OpaqueError>,
    resource: RubyResource,
) -> Result<String, FetchFailure> {
    let request = build_request(resource).map_err(FetchFailure::fatal)?;
    let response = client
        .serve(request)
        .await
        .map_err(|err| FetchFailure::retryable(anyhow!("network error: {err}")))?;

    let status = response.status();
    if !status.is_success() {
        let error = anyhow!(
            "{} request failed with status {}: {}",
            resource.label(),
            status.as_u16(),
            status.canonical_reason().unwrap_or("Unknown")
        );
        return Err(FetchFailure {
            retryable: should_retry_status(status),
            error,
        });
    }

    let body = response
        .into_body()
        .collect()
        .await
        .map_err(|err| {
            FetchFailure::retryable(anyhow!("collecting {} body: {err}", resource.label()))
        })?
        .to_bytes();
    String::from_utf8(body.to_vec())
        .with_context(|| format!("decoding {} as UTF-8", resource.label()))
        .map_err(FetchFailure::fatal)
}

/// Up to [`MAX_FETCH_ATTEMPTS`] tries, waiting about [`INITIAL_BACKOFF`],
/// then twice that, between them.
fn fetch_backoff() -> ExponentialBackoff {
    ExponentialBackoff::new()
        .max_attempts(MAX_FETCH_ATTEMPTS)
        .base_delay_ms(crate::util::millis(INITIAL_BACKOFF))
        .jitter_factor(0.2)
}

pub async fn fetch_ruby_status() -> Result<RubyStatus> {
    let client = build_client();
    let branches = fetch_yaml(&client, RubyResource::Branches).await?;
    let releases = fetch_yaml(&client, RubyResource::Releases).await?;
    Ok(RubyStatus::from_sources(branches, releases))
}

/// A `YYYY-MM-DD` date, as ruby-lang.org writes them; anything else is no
/// date.
fn parse_date(raw: Option<String>) -> Option<Date> {
    raw.and_then(|value| Date::strptime("%Y-%m-%d", &value).ok())
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

    security.sort_by_key(|branch| branch.expected_eol_date.unwrap_or(Date::MAX));
    eol.sort_by_key(|branch| branch.eol_date.unwrap_or(Date::MIN));
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
