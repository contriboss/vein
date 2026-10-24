use super::*;
#[cfg(feature = "gems")]
use crate::test_support::build_test_gem;
#[cfg(any(feature = "crates", feature = "npm"))]
use crate::test_support::tar_gz;

/// The opening of `state_machines` 0.202.0's real README.
#[cfg(feature = "gems")]
const STATE_MACHINES_README: &[u8] = b"![Build Status](https://github.com/state-machines/state_machines/actions/workflows/ruby.yml/badge.svg)\n\n# State Machines\n\nState Machines adds support for creating state machines for attributes on any Ruby class.\n";

#[cfg(feature = "gems")]
#[test]
fn a_gem_readme_sits_at_the_root_of_its_data() {
    let gem = build_test_gem(
        "--- !ruby/object:Gem::Specification\nname: state_machines\n",
        &[
            ("lib/state_machines.rb", b"module StateMachines; end\n"),
            ("lib/README.md", b"# not this one\n"),
            ("README.md", STATE_MACHINES_README),
        ],
    );
    let found = gem_readme(&std::fs::read(gem.path()).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(found.file, "README.md");
    assert!(found.text.contains("# State Machines"), "{}", found.text);
}

/// An archive's files: path, contents.
#[cfg(feature = "crates")]
type Files<'a> = &'a [(&'a str, &'a [u8])];

#[cfg(feature = "crates")]
#[test]
fn a_crate_readme_is_the_one_its_manifest_names() {
    let manifest = |readme: &str| {
        format!("[package]\nname = \"state-machines\"\nversion = \"0.32.1\"\n{readme}\n")
    };
    let cases: [(&str, Files<'_>, Option<&str>); 4] = [
        (
            "named",
            &[
                ("docs/OVERVIEW.md", b"# Overview\n"),
                ("README.md", b"# Root\n"),
            ],
            Some("# Overview\n"),
        ),
        ("readme = false", &[("README.md", b"# Root\n")], None),
        (
            "no manifest key",
            &[("README.md", b"# Root\n")],
            Some("# Root\n"),
        ),
        (
            "best of several",
            &[("README.txt", b"txt\n"), ("readme.md", b"# md\n")],
            Some("# md\n"),
        ),
    ];
    for (case, files, expected) in cases {
        let key = match case {
            "named" => "readme = \"docs/OVERVIEW.md\"",
            "readme = false" => "readme = false",
            _ => "",
        };
        let manifest = manifest(key);
        let mut entries: Vec<(String, &[u8])> = vec![(
            "state-machines-0.32.1/Cargo.toml".into(),
            manifest.as_bytes(),
        )];
        entries.extend(
            files
                .iter()
                .map(|(path, body)| (format!("state-machines-0.32.1/{path}"), *body)),
        );
        let entries: Vec<(&str, &[u8])> = entries.iter().map(|(p, b)| (p.as_str(), *b)).collect();
        let found = crate_readme(&tar_gz(&entries)).unwrap();
        assert_eq!(found.map(|f| f.text).as_deref(), expected, "{case}");
    }
}

#[cfg(feature = "npm")]
#[test]
fn an_npm_readme_sits_under_the_package_directory() {
    let tarball = tar_gz(&[
        (
            "package/package.json",
            br#"{"name":"left-pad","version":"1.3.0"}"#,
        ),
        ("package/README", b"plain\n"),
        ("package/README.md", b"## left-pad\n\nString left pad\n"),
        ("package/node_modules/x/README.md", b"# a dependency's\n"),
    ]);
    let found = npm_readme(&tarball).unwrap().unwrap();
    assert_eq!(
        (found.file.as_str(), found.text.as_str()),
        ("README.md", "## left-pad\n\nString left pad\n")
    );
    assert_eq!(
        npm_readme(&tar_gz(&[("package/index.js", b"")])).unwrap(),
        None
    );
}
