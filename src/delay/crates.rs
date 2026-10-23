//! The delay on the crates.io sparse index: one JSON object per version
//! and line, dated by `pubtime` (crates.io records it for every publish
//! since 2025; older entries have none).

use jiff::Timestamp;
use serde::Deserialize;
use vein_adapter::{AssetKind, CacheBackend};

use super::Listed;
use crate::config::DelayPolicyConfig;

#[derive(Deserialize)]
struct Entry<'a> {
    #[serde(borrow)]
    vers: &'a str,
    #[serde(default)]
    pubtime: Option<Timestamp>,
}

/// The sparse index file of crate `name` without the versions the policy
/// holds or blocks.
pub async fn filter_index(
    config: &DelayPolicyConfig,
    index: &CacheBackend,
    name: &str,
    file: Vec<u8>,
) -> Vec<u8> {
    super::filter_lines(config, index, AssetKind::Crate, name, file, parse_line).await
}

/// One version entry; `None` for a line that is not one.
#[must_use]
pub fn parse_line(line: &str) -> Option<Listed<'_>> {
    let entry: Entry<'_> = serde_json::from_str(line.trim_end()).ok()?;
    Some(Listed {
        version: entry.vers,
        platform: None,
        published_at: entry.pubtime,
    })
}

#[cfg(test)]
mod tests;
