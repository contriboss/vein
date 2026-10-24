use super::*;

#[test]
fn packages_are_checked_before_they_reach_a_path() {
    let ok = |kind, name: &str, version: &str| Package::new(kind, name, version).is_ok();
    for (kind, name, version, expected) in [
        (AssetKind::Gem, "state_machines", "0.202.0", true),
        (AssetKind::Crate, "state-machines", "0.32.1", true),
        (AssetKind::Crate, "serde_json", "1.0.154", true),
        (AssetKind::Crate, "tokio", "1.0.0-rc.1+build.5", true),
        (AssetKind::NpmPackage, "@types/node", "26.6.5", true),
        (AssetKind::NpmPackage, "left-pad", "1.3.0", true),
        (AssetKind::Gem, "../etc", "1", false),
        (AssetKind::Gem, "@types/node", "1", false),
        (AssetKind::NpmPackage, "@types/../x", "1", false),
        (AssetKind::NpmPackage, ".hidden", "1", false),
        (AssetKind::Crate, "serde", "1.0/../..", false),
        (AssetKind::Crate, "serde", "", false),
    ] {
        assert_eq!(
            ok(kind, name, version),
            expected,
            "{kind:?} {name} {version}"
        );
    }
}

#[test]
fn a_gem_version_names_its_native_build() {
    let native = Package::new(AssetKind::Gem, "nokogiri", "1.18.10-x86_64-linux-gnu").unwrap();
    assert_eq!(
        (native.version.as_str(), native.platform.as_deref()),
        ("1.18.10", Some("x86_64-linux-gnu"))
    );
    assert_eq!(
        native.download_path(),
        "/gems/nokogiri-1.18.10-x86_64-linux-gnu.gem"
    );
    let rc = Package::new(AssetKind::NpmPackage, "@types/node", "27.0.0-rc.1").unwrap();
    assert_eq!(rc.platform, None, "a semver prerelease is no platform");
    assert_eq!(rc.download_path(), "/@types/node/-/node-27.0.0-rc.1.tgz");
    assert_eq!(
        Package::new(AssetKind::Crate, "state-machines", "0.32.1")
            .unwrap()
            .download_path(),
        "/api/v1/crates/state-machines/0.32.1/download"
    );
}
