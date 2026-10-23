//! `RubyGems` version ordering, after `Gem::Version#<=>`.

use std::cmp::Ordering;

/// One run of digits or letters. Declaration order is the ordering: a letter
/// run (a prerelease tag such as `pre` or `rc`) sorts before any number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Segment<'a> {
    Tag(&'a str),
    Number(u64),
}

/// Orders `RubyGems` version strings: numeric segments compare as numbers,
/// prereleases sort before their release, and missing segments count as zero
/// (`1.0 == 1.0.0`, `1.9 < 1.10`, `1.0.0.rc1 < 1.0.0`).
#[must_use]
pub fn compare_gem_versions(a: &str, b: &str) -> Ordering {
    let (a, b): (Vec<_>, Vec<_>) = (segments(a).collect(), segments(b).collect());
    let zero = Segment::Number(0);
    (0..a.len().max(b.len()))
        .map(|i| {
            let lhs = a.get(i).unwrap_or(&zero);
            let rhs = b.get(i).unwrap_or(&zero);
            lhs.cmp(rhs)
        })
        .find(|ordering| ordering.is_ne())
        .unwrap_or(Ordering::Equal)
}

/// A semver prerelease (`7.1.0-beta`, `1.0.0-rc.1`), as crates.io and npm
/// spell them: a `-` before any `+build` metadata.
#[must_use]
pub fn is_semver_prerelease(version: &str) -> bool {
    version
        .split('+')
        .next()
        .is_some_and(|core| core.contains('-'))
}

/// Sorts `items` newest first by the `RubyGems` order of `version(item)`.
pub fn sort_newest_first<T>(items: &mut [T], version: impl Fn(&T) -> &str) {
    items.sort_by(|a, b| compare_gem_versions(version(b), version(a)));
}

fn segments(version: &str) -> impl Iterator<Item = Segment<'_>> {
    version
        .as_bytes()
        .chunk_by(|a, b| {
            a.is_ascii_digit() == b.is_ascii_digit()
                && a.is_ascii_alphabetic() == b.is_ascii_alphabetic()
        })
        .filter_map(|run| {
            let text = std::str::from_utf8(run).ok()?;
            if run[0].is_ascii_digit() {
                Some(Segment::Number(text.parse().unwrap_or(u64::MAX)))
            } else if run[0].is_ascii_alphabetic() {
                Some(Segment::Tag(text))
            } else {
                None
            }
        })
}

#[cfg(test)]
mod tests;
