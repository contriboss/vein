use vein_adapter::{Advisory, AssetKind};

use super::*;

const PAGEANT: &str = include_str!("../../tests/fixtures/advisories/crate-pageant.json");
const SHOPIFY_APP: &str = include_str!("../../tests/fixtures/advisories/gem-shopify_app.json");
const TWO_RANGES: &str = include_str!("../../tests/fixtures/advisories/npm-two-ranges.json");
const FROM_ZERO: &str = include_str!("../../tests/fixtures/advisories/npm-from-zero.json");
const LAST_AFFECTED: &str = include_str!("../../tests/fixtures/advisories/last-affected.json");
const WITHDRAWN: &str = include_str!("../../tests/fixtures/advisories/withdrawn.json");
const PYPI_ONLY: &str = include_str!("../../tests/fixtures/advisories/pypi-only.json");

fn advisory(json: &str) -> Advisory {
    parse(json).unwrap().expect("an advisory vein keeps")
}

#[test]
fn reads_what_an_agent_needs() {
    let advisory = advisory(SHOPIFY_APP);
    assert_eq!(advisory.id, "GHSA-6j52-38f8-qhxr");
    assert_eq!(advisory.severity.as_deref(), Some("HIGH"));
    assert_eq!(advisory.packages.len(), 1);
    let package = &advisory.packages[0];
    assert_eq!((package.kind, package.name.as_str()), (AssetKind::Gem, "shopify_app"));
    assert_eq!(requirements(&package.affected), [">= 22.1.0, < 23.0.3"]);
}

/// A package reported once per range is one package with both intervals.
#[test]
fn ranges_of_one_package_are_merged() {
    let advisory = advisory(TWO_RANGES);
    assert_eq!(advisory.packages.len(), 1);
    assert_eq!(
        requirements(&advisory.packages[0].affected),
        [">= 3.25.0, < 3.90.0", ">= 4.0.0-canary.0, < 4.0.0-canary.34"]
    );
}

/// `introduced: "0"` is every version; git ranges are commits, not
/// versions; listed versions are kept; severity is upper-cased.
#[test]
fn reads_every_shape_of_range() {
    assert_eq!(requirements(&advisory(FROM_ZERO).packages[0].affected), ["< 1.1.1"]);
    let advisory = advisory(LAST_AFFECTED);
    assert_eq!(advisory.severity.as_deref(), Some("CRITICAL"));
    assert_eq!(
        requirements(&advisory.packages[0].affected),
        [">= 1.2.0, <= 1.4.2", "= 0.9.9"]
    );
}

#[test]
fn withdrawn_and_foreign_advisories_are_left_out() {
    assert_eq!(parse(WITHDRAWN).unwrap(), None);
    assert_eq!(parse(PYPI_ONLY).unwrap(), None);
    assert!(parse("{").is_err());
}

#[test]
fn matches_versions_and_names_the_fix() {
    let pageant = &advisory(PAGEANT).packages[0].affected;
    let fixed = Some(Hit {
        fixed_in: Some("0.2.5".to_owned()),
    });
    assert_eq!(affects(AssetKind::Crate, pageant, "0.2.0"), fixed);
    assert_eq!(affects(AssetKind::Crate, pageant, "0.2.4"), fixed);
    assert_eq!(affects(AssetKind::Crate, pageant, "0.2.5"), None);
    assert_eq!(affects(AssetKind::Crate, pageant, "0.1.9"), None);

    let demo = &advisory(LAST_AFFECTED).packages[0].affected;
    assert_eq!(
        affects(AssetKind::Crate, demo, "1.4.2"),
        Some(Hit { fixed_in: None })
    );
    assert_eq!(affects(AssetKind::Crate, demo, "1.4.3"), None);
    assert_eq!(
        affects(AssetKind::Crate, demo, "0.9.9"),
        Some(Hit { fixed_in: None })
    );
}

/// Prereleases order as their registry orders them.
#[test]
fn prereleases_order_per_registry() {
    let payload = &advisory(TWO_RANGES).packages[0].affected;
    let hit = |version| affects(AssetKind::NpmPackage, payload, version);
    assert_eq!(
        hit("4.0.0-canary.9").and_then(|hit| hit.fixed_in).as_deref(),
        Some("4.0.0-canary.34")
    );
    assert_eq!(hit("4.0.0-canary.34"), None);
    assert_eq!(hit("4.0.0"), None);
    assert_eq!(hit("3.24.9"), None);

    let shopify = &advisory(SHOPIFY_APP).packages[0].affected;
    assert!(affects(AssetKind::Gem, shopify, "23.0.3.rc1").is_some());
    assert!(affects(AssetKind::Gem, shopify, "23.0.3").is_none());
}
