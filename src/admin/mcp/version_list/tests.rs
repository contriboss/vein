use super::*;
use vein_adapter::is_semver_prerelease;

/// npm's `typescript`, abridged: nightlies newer than every release.
const TYPESCRIPT: &[&str] = &[
    "7.1.0-dev.20261023.1",
    "7.1.0-dev.20261022.1",
    "7.1.0-beta",
    "7.0.2",
    "7.0.1-rc",
    "7.0.1",
    "7.0.0",
    "6.0.3",
    "6.0.2",
    "6.0.1",
];

#[test]
fn semver_prereleases_have_a_dash_before_build_metadata() {
    for (version, expected) in [
        ("7.1.0-beta", true),
        ("1.0.0-rc.1", true),
        ("4.6.2-insiders.20220225", true),
        ("1.0.0+build-7", false),
        ("1.0.0-alpha+build", true),
        ("26.6.5", false),
    ] {
        assert_eq!(is_semver_prerelease(version), expected, "{version}");
    }
}

#[test]
fn summaries_name_five_releases_past_the_nightlies() {
    let releases = newest_releases(TYPESCRIPT.iter().copied(), is_semver_prerelease);
    assert_eq!(releases, ["7.0.2", "7.0.1", "7.0.0", "6.0.3", "6.0.2"]);
    assert_eq!(
        newer_prerelease(TYPESCRIPT.iter().copied(), is_semver_prerelease).as_deref(),
        Some("7.1.0-dev.20261023.1")
    );
    assert_eq!(
        newer_prerelease(TYPESCRIPT[3..].iter().copied(), is_semver_prerelease),
        None,
        "7.0.1-rc is older than 7.0.2"
    );
}

#[test]
fn full_lists_keep_every_version_apart() {
    let list = VersionList::split(TYPESCRIPT.iter().copied(), is_semver_prerelease);
    assert_eq!(list.versions.len(), 6);
    assert_eq!(
        list.prereleases,
        [
            "7.1.0-dev.20261023.1",
            "7.1.0-dev.20261022.1",
            "7.1.0-beta",
            "7.0.1-rc"
        ]
    );
}
