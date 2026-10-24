use super::*;

#[test]
fn served_registries_are_the_features_built_in() {
    let expected: Vec<AssetKind> = [
        (cfg!(feature = "gems"), AssetKind::Gem),
        (cfg!(feature = "crates"), AssetKind::Crate),
        (cfg!(feature = "npm"), AssetKind::NpmPackage),
    ]
    .into_iter()
    .filter_map(|(on, kind)| on.then_some(kind))
    .collect();
    assert_eq!(served(), expected);
    assert_eq!(
        built(AssetKind::Spec),
        built(AssetKind::Gem),
        "specs are gems"
    );
}
