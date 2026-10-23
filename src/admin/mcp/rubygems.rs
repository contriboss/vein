//! `RubyGems` compact-index `info` files as the proxy caches them.
//!
//! Format: a `---` line, then one line per published build,
//! `VERSION[-PLATFORM] DEPS|checksum:...,ruby:...`. Yanked versions are
//! removed from it by the registry.

use std::collections::BTreeSet;

use vein_adapter::sort_newest_first;

/// rubygems.org gem names: an ASCII letter or digit, then letters, digits,
/// `.`, `_`, `-`. Also the guard before a name goes into a proxy path, so
/// `..` never does.
pub fn is_valid_name(name: &str) -> bool {
    name.len() <= 255
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Published versions, newest first, once each (platform builds of a
/// version count as that version).
pub fn versions(info: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(info);
    let unique: BTreeSet<&str> = text
        .lines()
        .filter(|line| !line.starts_with("---"))
        .filter_map(|line| line.split_whitespace().next())
        .map(|build| build.split_once('-').map_or(build, |(version, _)| version))
        .filter(|version| !version.is_empty())
        .collect();
    let mut versions: Vec<String> = unique.into_iter().map(str::to_string).collect();
    sort_newest_first(&mut versions, String::as_str);
    versions
}

/// A `RubyGems` prerelease (`3.0.0.rc1`, `3.0.0.beta1`): any letter.
pub fn is_prerelease(version: &str) -> bool {
    version.chars().any(|c| c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests;
