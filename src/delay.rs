//! The release delay: one policy for gems, crates and npm packages.
//!
//! A new version stays out of the index vein serves until its delay is
//! over, counted from when the registry published it: rubygems.org's
//! compact index carries `created_at`, the crates.io sparse index
//! `pubtime`, an npm packument its `time` map. Admins approve or block
//! single versions; config pins release one early. Each registry's module
//! parses its index format and drops what the policy holds; this module
//! decides.

pub mod crates;
pub mod gems;
pub mod npm;

use jiff::Timestamp;
use rama::telemetry::tracing::warn;
use vein_adapter::{
    AssetKey, AssetKind, CacheBackend, CacheBackendTrait, PackageVersion, VersionStatus,
    calculate_availability,
};

use crate::config::DelayPolicyConfig;

/// The body of the 403 a blocked version's download gets.
pub const BLOCKED: &str = "This version is blocked by vein's release delay policy.\n";

/// One version as a registry's index lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Listed<'a> {
    pub version: &'a str,
    /// Gems only: the native platform; `None` for `ruby`.
    pub platform: Option<&'a str>,
    /// When the registry published it; `None` for entries older than the
    /// registry's record of it.
    pub published_at: Option<Timestamp>,
}

/// What the policy makes of one listed version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Visible,
    /// Hidden until then.
    Held(Timestamp),
    /// Hidden, and its downloads refused.
    Blocked,
}

impl Verdict {
    #[must_use]
    pub const fn is_visible(self) -> bool {
        matches!(self, Self::Visible)
    }
}

/// The verdict on `listed`, given its tracked row (an admin's decision, or
/// an earlier hold) and the time.
#[must_use]
pub fn decide(
    config: &DelayPolicyConfig,
    kind: AssetKind,
    name: &str,
    listed: Listed<'_>,
    row: Option<&PackageVersion>,
    now: Timestamp,
) -> Verdict {
    match row.map(|row| row.status) {
        Some(VersionStatus::Blocked) => return Verdict::Blocked,
        Some(VersionStatus::Available | VersionStatus::Pinned) => return Verdict::Visible,
        Some(VersionStatus::Quarantine) | None => {}
    }
    if config.pin_reason(kind, name, listed.version).is_some()
        || config.delay_days_for(kind, name) == 0
    {
        return Verdict::Visible;
    }
    // A registry that does not date a version: when vein first held it,
    // else it predates the registry recording publish times.
    let Some(published) = listed
        .published_at
        .or_else(|| row.map(|row| row.published_at))
    else {
        return Verdict::Visible;
    };
    let until = calculate_availability(published, &config.policy_for(kind, name));
    if now >= until {
        Verdict::Visible
    } else {
        Verdict::Held(until)
    }
}

/// Verdicts on every version of package `name` in `listed` order, the held
/// ones recorded so admins can see and decide on them; `None` when the
/// policy is off.
///
/// Fails safe: without the tracked rows (database down) it still holds by
/// publish time, only admin decisions are missing.
pub async fn screen(
    config: &DelayPolicyConfig,
    index: &CacheBackend,
    kind: AssetKind,
    name: &str,
    listed: &[Listed<'_>],
) -> Option<Vec<Verdict>> {
    if !config.enabled {
        return None;
    }
    let rows = index.package_versions(kind, name).await.unwrap_or_else(|err| {
        warn!(error = %err, package = name, "delay: tracked versions unavailable, holding by publish time only");
        Vec::new()
    });
    let now = Timestamp::now();
    let mut held = Vec::new();
    let verdicts: Vec<Verdict> = listed
        .iter()
        .map(|listed| {
            let row = rows.iter().find(|row| {
                row.version == listed.version && row.platform.as_deref() == listed.platform
            });
            let verdict = decide(config, kind, name, *listed, row, now);
            if let Verdict::Held(until) = verdict {
                // `decide` holds only what it can date, from the index or the row.
                let published = listed
                    .published_at
                    .or_else(|| row.map(|row| row.published_at));
                held.push(held_row(
                    kind,
                    name,
                    listed,
                    published.unwrap_or(now),
                    until,
                    now,
                ));
            }
            verdict
        })
        .collect();

    if let Err(err) = index.record_held(&held).await {
        warn!(error = %err, package = name, "delay: could not record held versions");
    }
    Some(verdicts)
}

/// `body` without the lines the policy holds or blocks, for line-based
/// indexes (compact `info`, sparse index): every other line, header and
/// line endings included, comes back as it was.
async fn filter_lines(
    config: &DelayPolicyConfig,
    index: &CacheBackend,
    kind: AssetKind,
    name: &str,
    body: Vec<u8>,
    parse_line: fn(&str) -> Option<Listed<'_>>,
) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(&body) else {
        return body;
    };
    let lines: Vec<(&str, Option<Listed<'_>>)> = text
        .split_inclusive('\n')
        .map(|line| (line, parse_line(line)))
        .collect();
    let listed: Vec<Listed<'_>> = lines.iter().filter_map(|(_, listed)| *listed).collect();
    let Some(verdicts) = screen(config, index, kind, name, &listed).await else {
        return body;
    };
    let mut verdicts = verdicts.into_iter();
    lines
        .into_iter()
        .filter(|(_, listed)| listed.is_none() || verdicts.next().is_some_and(Verdict::is_visible))
        .map(|(line, _)| line)
        .collect::<String>()
        .into_bytes()
}

/// Marks held versions whose delay is over as available.
///
/// The index already serves them; this keeps the dashboard and
/// `vein://quarantine` in step. [`crate::jobs`] runs it hourly, `vein
/// quarantine promote` on demand.
pub async fn promote_now(index: &CacheBackend) -> anyhow::Result<u64> {
    use anyhow::Context as _;
    index
        .promote_expired_quarantines(Timestamp::now())
        .await
        .context("promoting expired quarantines")
}

/// Whether an admin blocked this exact version: its downloads are refused.
/// Held versions stay downloadable (a lockfile may already name one).
pub async fn is_blocked(
    config: &DelayPolicyConfig,
    index: &CacheBackend,
    key: &AssetKey<'_>,
) -> bool {
    if !config.enabled {
        return false;
    }
    match index.package_versions(key.kind, key.name).await {
        Ok(rows) => rows.iter().any(|row| {
            row.status == VersionStatus::Blocked
                && row.version == key.version
                && row.platform.as_deref() == key.platform
        }),
        Err(err) => {
            warn!(error = %err, package = key.name, "delay: block list unavailable, serving the download");
            false
        }
    }
}

fn held_row(
    kind: AssetKind,
    name: &str,
    listed: &Listed<'_>,
    published_at: Timestamp,
    until: Timestamp,
    now: Timestamp,
) -> PackageVersion {
    PackageVersion {
        id: 0,
        kind,
        name: name.to_owned(),
        version: listed.version.to_owned(),
        platform: listed.platform.map(str::to_owned),
        published_at,
        available_after: until,
        status: VersionStatus::Quarantine,
        status_reason: Some("auto".to_owned()),
        created_at: now,
        updated_at: now,
    }
}

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
