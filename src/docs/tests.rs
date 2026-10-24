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
        // A registry built out has no packages at all.
        assert_eq!(
            ok(kind, name, version),
            expected && registry::built(kind),
            "{kind:?} {name} {version}"
        );
    }
}

#[test]
fn a_registry_built_out_says_so() {
    for kind in [AssetKind::Gem, AssetKind::Crate, AssetKind::NpmPackage] {
        if registry::built(kind) {
            continue;
        }
        let err = Package::new(kind, "state_machines", "1.0.0").unwrap_err();
        assert_eq!(err.downcast_ref::<NotBuilt>(), Some(&NotBuilt(kind)));
    }
}

#[cfg(feature = "gems")]
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
}

#[test]
fn archives_are_where_clients_download_them() {
    for (kind, name, version, path) in [
        (
            AssetKind::NpmPackage,
            "@types/node",
            "27.0.0-rc.1",
            "/@types/node/-/node-27.0.0-rc.1.tgz",
        ),
        (
            AssetKind::Crate,
            "state-machines",
            "0.32.1",
            "/api/v1/crates/state-machines/0.32.1/download",
        ),
    ] {
        if !registry::built(kind) {
            continue;
        }
        let package = Package::new(kind, name, version).unwrap();
        assert_eq!(package.platform, None, "only gems have platforms: {name}");
        assert_eq!(package.download_path(), path);
    }
}
