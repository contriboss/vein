use vein_adapter::{Advisory, AssetKind};

use super::*;

fn advisories() -> Vec<Advisory> {
    [
        include_str!("../../../../tests/fixtures/advisories/npm-two-ranges.json"),
        include_str!("../../../../tests/fixtures/advisories/npm-from-zero.json"),
    ]
    .map(|json| crate::advisories::parse(json).unwrap().unwrap())
    .into()
}

/// Only advisories hitting a listed version show, each naming which (once).
#[test]
fn notes_name_the_listed_versions_they_affect() {
    let notes = for_versions(
        AssetKind::NpmPackage,
        &advisories(),
        &["3.90.0", "3.89.1", "4.0.0-canary.3", "3.89.1"],
    );
    assert_eq!(notes.len(), 1, "filecoin-pin's advisory is about 1.1.0");
    assert_eq!(notes[0].id, "GHSA-mc8m-rr6c-r5qr");
    assert_eq!(notes[0].affects, ["3.89.1", "4.0.0-canary.3"]);
    assert_eq!(notes[0].severity.as_deref(), Some("moderate"));
    assert_eq!(notes[0].cve, ["CVE-2026-93363"]);
    assert_eq!(notes[0].fixed_in, None);

    assert_eq!(
        for_versions(AssetKind::NpmPackage, &advisories(), &["9.9.9"]),
        []
    );
}

/// One version's notes name the fix of the interval it falls in.
#[test]
fn a_version_learns_its_fix() {
    let notes = for_version(AssetKind::NpmPackage, &advisories(), "4.0.0-canary.3");
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].fixed_in.as_deref(), Some("4.0.0-canary.34"));
    assert_eq!(notes[0].affects, Vec::<String>::new());
}

#[test]
fn the_full_list_leaves_summaries_out() {
    let notes = every(&advisories());
    assert_eq!(notes.len(), 2);
    assert!(notes.iter().all(|note| note.summary.is_none()));
    assert_eq!(notes[1].vulnerable, ["< 1.1.1"]);
}
