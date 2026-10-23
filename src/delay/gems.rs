//! The delay on rubygems.org's compact index.
//!
//! `info/<name>` lines are `VERSION[-PLATFORM] DEPS|REQUIREMENTS`, the
//! requirements ending in `created_at:<RFC 3339>` (e.g. `8.1.4 railties:=
//! 8.1.4|checksum:5f3e,ruby:>= 3.2.0,created_at:2026-09-24T14:22:14Z`).

use jiff::Timestamp;
use vein_adapter::{AssetKind, CacheBackend};

use super::Listed;
use crate::config::DelayPolicyConfig;

/// `info` without the lines the policy holds or blocks for gem `name`.
pub async fn filter_info(
    config: &DelayPolicyConfig,
    index: &CacheBackend,
    name: &str,
    info: Vec<u8>,
) -> Vec<u8> {
    super::filter_lines(config, index, AssetKind::Gem, name, info, parse_line).await
}

/// One version line of an `info` file; `None` for the `---` header, blank
/// lines and anything else that is not a version.
#[must_use]
pub fn parse_line(line: &str) -> Option<Listed<'_>> {
    let (build, requirements) = line.trim_end().split_once('|')?;
    let build = build.split_whitespace().next()?;
    // RubyGems versions hold no `-`: the first one starts the platform.
    let (version, platform) = match build.split_once('-') {
        Some((version, platform)) => (version, Some(platform)),
        None => (build, None),
    };
    let published_at = requirements
        .split(',')
        .find_map(|requirement| requirement.strip_prefix("created_at:"))
        .and_then(|at| at.parse::<Timestamp>().ok());
    Some(Listed {
        version,
        platform: platform.filter(|platform| *platform != "ruby"),
        published_at,
    })
}

#[cfg(test)]
mod tests;
