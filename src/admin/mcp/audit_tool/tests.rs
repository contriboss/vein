use vein_adapter::AssetKind;

use super::*;

fn note(fixed_in: Option<&str>) -> Note {
    Note {
        id: "GHSA-test".into(),
        cve: Vec::new(),
        severity: None,
        summary: Some("what went wrong".into()),
        vulnerable: Vec::new(),
        affects: Vec::new(),
        fixed_in: fixed_in.map(Into::into),
    }
}

#[test]
fn many_advisories_lose_their_summaries() {
    let mut few: Vec<Note> = (0..SUMMARIES_UP_TO).map(|_| note(None)).collect();
    shorten(&mut few);
    assert!(few.iter().all(|note| note.summary.is_some()));

    let mut many: Vec<Note> = (0..=SUMMARIES_UP_TO).map(|_| note(None)).collect();
    shorten(&mut many);
    assert!(many.iter().all(|note| note.summary.is_none()));
}

/// The fix of every advisory, by the registry's order; none if one has no
/// fix.
#[test]
fn upgrades_to_the_highest_fix() {
    let notes = [
        note(Some("2.2.9")),
        note(Some("2.2.23")),
        note(Some("2.2.8.1")),
    ];
    assert_eq!(
        upgrade_to(AssetKind::Gem, &notes).as_deref(),
        Some("2.2.23")
    );
    let unfixed = [note(Some("2.2.23")), note(None)];
    assert_eq!(upgrade_to(AssetKind::Gem, &unfixed), None);
}
