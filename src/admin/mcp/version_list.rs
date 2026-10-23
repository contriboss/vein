//! How resources list versions: a package names its newest few releases
//! (and a prerelease newer than all of them); `.../versions` has every one.

use std::ops::Not;

use serde::Serialize;

/// Releases a package resource lists.
pub const SUMMARY_RELEASES: usize = 5;

/// A semver prerelease (`7.1.0-beta`, `1.0.0-rc.1`), as crates.io and npm
/// spell them: a `-` before any `+build` metadata.
pub fn is_semver_prerelease(version: &str) -> bool {
    version
        .split('+')
        .next()
        .is_some_and(|core| core.contains('-'))
}

/// The newest [`SUMMARY_RELEASES`] releases of `newest_first`.
pub fn newest_releases<'a>(
    newest_first: impl IntoIterator<Item = &'a str>,
    is_prerelease: impl Fn(&str) -> bool,
) -> Vec<String> {
    newest_first
        .into_iter()
        .filter(|version| !is_prerelease(version))
        .take(SUMMARY_RELEASES)
        .map(str::to_owned)
        .collect()
}

/// The newest version, when it is a prerelease: newer than every release,
/// so what opting into prereleases would install.
pub fn newer_prerelease<'a>(
    newest_first: impl IntoIterator<Item = &'a str>,
    is_prerelease: impl Fn(&str) -> bool,
) -> Option<String> {
    newest_first
        .into_iter()
        .next()
        .filter(|version| is_prerelease(version))
        .map(str::to_owned)
}

/// Every version of a package, for `.../versions`.
#[derive(Serialize, Default)]
pub struct VersionList {
    /// Releases, newest first; each is at `.../versions/{v}`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub versions: Vec<String>,
    /// Prereleases, newest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prereleases: Vec<String>,
    /// crates.io: the yanked ones.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub yanked: Vec<String>,
    /// npm: every version is deprecated; `deprecated` is then left out.
    #[serde(skip_serializing_if = "Not::not")]
    pub all_deprecated: bool,
    /// npm: the deprecated ones.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub deprecated: Vec<String>,
}

impl VersionList {
    /// `newest_first` split into releases and prereleases.
    pub fn split<'a>(
        newest_first: impl IntoIterator<Item = &'a str>,
        is_prerelease: impl Fn(&str) -> bool,
    ) -> Self {
        let (prereleases, versions) = newest_first
            .into_iter()
            .map(str::to_owned)
            .partition(|version| is_prerelease(version));
        Self {
            versions,
            prereleases,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests;
