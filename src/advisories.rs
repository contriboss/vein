//! Security advisories, from GitHub's reviewed advisory database.
//!
//! A recurring job imports them into Postgres; `/mcp` matches them against
//! the versions an agent reads. Nothing is pushed: an advisory costs a
//! lookup only when someone looks at a package it is about.

mod osv;
mod sync;

use std::cmp::Ordering;

use vein_adapter::{Affected, AssetKind, Interval, compare_gem_versions};

pub use osv::parse;
pub use sync::{SOURCE, TASK, sync};

/// Orders two versions of registry `kind`: semver for crates and npm (the
/// `RubyGems` order when either is not semver), `RubyGems`' own for gems.
#[must_use]
pub fn compare(kind: AssetKind, a: &str, b: &str) -> Ordering {
    match kind {
        AssetKind::Crate | AssetKind::NpmPackage => {
            match (semver::Version::parse(a), semver::Version::parse(b)) {
                (Ok(a), Ok(b)) => a.cmp(&b),
                _ => compare_gem_versions(a, b),
            }
        }
        AssetKind::Gem | AssetKind::Spec => compare_gem_versions(a, b),
    }
}

/// Whether `version` is in `affected`; if so, the release that fixes it,
/// when its interval names one.
#[must_use]
pub fn affects(kind: AssetKind, affected: &Affected, version: &str) -> Option<Hit> {
    if affected.versions.iter().any(|listed| listed == version) {
        return Some(Hit { fixed_in: None });
    }
    affected
        .intervals
        .iter()
        .find(|interval| contains(kind, interval, version))
        .map(|interval| Hit {
            fixed_in: interval.fixed.clone(),
        })
}

/// A version an advisory affects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// The first release out of the affected interval.
    pub fixed_in: Option<String>,
}

fn contains(kind: AssetKind, interval: &Interval, version: &str) -> bool {
    let at_least = |bound: &Option<String>| {
        bound
            .as_deref()
            .is_none_or(|bound| compare(kind, version, bound).is_ge())
    };
    let below = |bound: &str| compare(kind, version, bound).is_lt();
    let at_most = |bound: &str| compare(kind, version, bound).is_le();
    at_least(&interval.introduced)
        && interval.fixed.as_deref().is_none_or(below)
        && interval.last_affected.as_deref().is_none_or(at_most)
}

/// `affected` as requirements, one per interval: `>= 22.1.0, < 23.0.3`,
/// `<= 1.1.0`, `all`; listed versions as `= 1.2.3`.
#[must_use]
pub fn requirements(affected: &Affected) -> Vec<String> {
    let intervals = affected.intervals.iter().map(|interval| {
        let parts: Vec<String> = [
            interval.introduced.as_ref().map(|v| format!(">= {v}")),
            interval.fixed.as_ref().map(|v| format!("< {v}")),
            interval.last_affected.as_ref().map(|v| format!("<= {v}")),
        ]
        .into_iter()
        .flatten()
        .collect();
        if parts.is_empty() {
            "all".to_owned()
        } else {
            parts.join(", ")
        }
    });
    let listed = affected.versions.iter().map(|v| format!("= {v}"));
    intervals.chain(listed).collect()
}

#[cfg(test)]
mod tests;
