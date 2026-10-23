use super::*;
use crate::test_support::{build_test_gem, digest_file};
use vein_adapter::DependencyKind;

#[tokio::test]
async fn parses_complete_metadata_payload() {
    let metadata_yaml = r#"---
name: test-gem
version: 1.2.3
authors:
  - Alice
  - Bob
licenses:
  - MIT
email: support@example.com
summary: "Test gem"
description: "Detailed description"
homepage: "https://example.test"
platform: ruby
metadata:
  source_code_uri: "https://git.example/test"
  documentation_uri: "https://docs.example/test"
  funding_uri: "https://example.com/sponsor"
dependencies:
  - name: activesupport
    type: runtime
    requirement:
      requirements:
        - - ">="
          - "7.0"
  - name: rake
    type: development
    requirement:
      requirements:
        - - "~>"
          - "13.0"
executables:
  - test-cli
extensions:
  - ext/test_extension.c
required_ruby_version:
  requirements:
    - - ">="
      - "2.7.0"
required_rubygems_version:
  requirements:
    - - ">="
      - "3.3.0"
rubygems_version: "3.4.7"
specification_version: 4
date: "2024-11-15"
"#;

    let gem_file = build_test_gem(
        metadata_yaml,
        &[
            ("lib/test.rb", b"puts 'hi'"),
            ("ext/native/ext.c", b"int hello(void) { return 42; }"),
        ],
    );
    let (size_bytes, sha256) = digest_file(gem_file.path());

    let metadata = extract_gem_metadata(
        gem_file.path(),
        "test-gem",
        "1.2.3",
        None,
        size_bytes,
        &sha256,
        None,
    )
    .await
    .expect("metadata extraction succeeds")
    .expect("metadata is present");

    assert_eq!(metadata.name, "test-gem");
    assert_eq!(metadata.version, "1.2.3");
    assert_eq!(metadata.platform, "ruby");
    assert_eq!(metadata.summary.as_deref(), Some("Test gem"));
    assert_eq!(
        metadata.description.as_deref(),
        Some("Detailed description")
    );
    assert_eq!(metadata.licenses, ["MIT"]);
    assert_eq!(metadata.authors, ["Alice", "Bob"]);
    assert_eq!(metadata.emails, ["support@example.com"]);
    assert_eq!(metadata.homepage.as_deref(), Some("https://example.test"));
    assert_eq!(
        metadata.documentation_url.as_deref(),
        Some("https://docs.example/test")
    );
    assert_eq!(
        metadata.source_code_url.as_deref(),
        Some("https://git.example/test")
    );
    assert_eq!(
        metadata.funding_url.as_deref(),
        Some("https://example.com/sponsor")
    );
    assert_eq!(metadata.dependencies.len(), 2);
    assert_eq!(metadata.dependencies[0].name, "activesupport");
    assert_eq!(metadata.dependencies[0].requirement, ">= 7.0");
    assert_eq!(metadata.dependencies[0].kind, DependencyKind::Runtime);
    assert_eq!(metadata.dependencies[1].name, "rake");
    assert_eq!(metadata.dependencies[1].kind, DependencyKind::Development);
    assert_eq!(metadata.executables, ["test-cli"]);
    assert_eq!(metadata.extensions, ["ext/test_extension.c"]);
    assert!(metadata.has_native_extensions);
    assert!(!metadata.has_embedded_binaries);
    assert_eq!(metadata.native_languages, ["C"]);
    assert_eq!(metadata.required_ruby_version.as_deref(), Some(">= 2.7.0"));
    assert_eq!(
        metadata.required_rubygems_version.as_deref(),
        Some(">= 3.3.0")
    );
    assert_eq!(metadata.rubygems_version.as_deref(), Some("3.4.7"));
    assert_eq!(metadata.specification_version, Some(4));
    assert_eq!(metadata.built_at.as_deref(), Some("2024-11-15"));
    assert_eq!(metadata.size_bytes, size_bytes);
    assert_eq!(metadata.sha256, sha256);
    assert_eq!(
        metadata.metadata["source_code_uri"],
        serde_json::Value::String("https://git.example/test".to_string())
    );
    let sbom = metadata.sbom.as_ref().expect("sbom present");
    assert_eq!(sbom["metadata"]["component"]["name"], "test-gem");
    assert_eq!(sbom["metadata"]["component"]["version"], "1.2.3");
    assert_eq!(
        sbom["metadata"]["component"]["purl"],
        serde_json::Value::String("pkg:gem/test-gem@1.2.3".to_string())
    );
}

#[tokio::test]
async fn detects_native_artifacts_in_data_archive() {
    let metadata_yaml = r"---
name: native-gem
version: 0.1.0
authors: Native Dev
licenses: []
";
    let gem_file = build_test_gem(
        metadata_yaml,
        &[
            ("ext/native/native.bundle", b"\x00\x01"),
            ("bin/tool", b"\x02"),
            ("lib/native.rb", b"puts 'native'"),
        ],
    );
    let (size_bytes, sha256) = digest_file(gem_file.path());

    let metadata = extract_gem_metadata(
        gem_file.path(),
        "native-gem",
        "0.1.0",
        Some("arm64-darwin"),
        size_bytes,
        &sha256,
        None,
    )
    .await
    .expect("metadata extraction succeeds")
    .expect("metadata is present");

    assert_eq!(metadata.platform, "arm64-darwin");
    assert!(
        metadata.has_native_extensions,
        "ext/ directory marks native"
    );
    assert!(
        metadata.has_embedded_binaries,
        "bundle or bin/* flags embedded binaries"
    );
    assert_eq!(metadata.extensions, [] as [std::string::String; 0]);
    assert_eq!(metadata.executables, [] as [std::string::String; 0]);
    assert_eq!(
        metadata.dependencies,
        [] as [vein_adapter::GemDependency; 0]
    );
    assert!(metadata.metadata.is_null());
    assert_eq!(metadata.native_languages, ["Native Binary"]);
    assert!(metadata.sbom.is_some());
}

#[tokio::test]
async fn returns_none_for_non_mapping_metadata() {
    let metadata_yaml = r#"--- "just a string""#;
    let gem_file = build_test_gem(metadata_yaml, &[]);
    let (size_bytes, sha256) = digest_file(gem_file.path());

    let result = extract_gem_metadata(
        gem_file.path(),
        "broken",
        "0.0.1",
        None,
        size_bytes,
        &sha256,
        None,
    )
    .await
    .expect("metadata extraction succeeds");

    assert!(result.is_none(), "non-mapping YAML should be ignored");
}

#[tokio::test]
async fn reuses_existing_sbom_when_available() {
    let metadata_yaml = r#"---
name: reusable
version: 0.3.0
authors: ["Cache"]
licenses: ["MIT"]
"#;
    let gem_file = build_test_gem(metadata_yaml, &[]);
    let (size_bytes, sha256) = digest_file(gem_file.path());

    let existing_sbom = serde_json::json!({
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "metadata": { "component": { "name": "reusable", "version": "0.3.0" } }
    });

    let metadata = extract_gem_metadata(
        gem_file.path(),
        "reusable",
        "0.3.0",
        None,
        size_bytes,
        &sha256,
        Some(existing_sbom.clone()),
    )
    .await
    .expect("metadata extraction succeeds")
    .expect("metadata is present");

    assert_eq!(
        metadata.sbom,
        Some(existing_sbom),
        "should reuse precomputed SBOM"
    );
}

/// rake 13.4.2 ships `doc/example/*.c`, and gems commonly ship
/// `bin/console` scripts: neither is native code.
#[tokio::test]
async fn sources_outside_ext_and_bin_scripts_are_not_native() {
    let metadata_yaml = "---\nname: rakeish\nversion: 1.0.0\nauthors: Dev\nlicenses: []\n";
    let gem_file = build_test_gem(
        metadata_yaml,
        &[
            ("doc/example/a.c", b"int main(void) { return 0; }"),
            ("test/fixtures/native.cc", b"int x;"),
            ("lib/rakeish.rb", b"module Rakeish; end"),
            ("exe/rakeish", b"#!/usr/bin/env ruby\nrequire 'rakeish'"),
            ("bin/console", b"#!/usr/bin/env ruby\nrequire 'irb'"),
        ],
    );
    let (size_bytes, sha256) = digest_file(gem_file.path());
    let metadata = extract_gem_metadata(
        gem_file.path(),
        "rakeish",
        "1.0.0",
        None,
        size_bytes,
        &sha256,
        None,
    )
    .await
    .expect("metadata extraction succeeds")
    .expect("metadata is present");

    assert_eq!(metadata.native_languages, Vec::<String>::new());
    assert!(!metadata.has_native_extensions);
    assert!(!metadata.has_embedded_binaries, "bin/console is a script");
}

#[tokio::test]
async fn languages_come_from_ext_and_bin_binaries_from_their_magic() {
    let metadata_yaml = "---\nname: rusty\nversion: 2.0.0\nauthors: Dev\nlicenses: []\n";
    let gem_file = build_test_gem(
        metadata_yaml,
        &[
            ("ext/rusty/Cargo.toml", b"[package]\nname = \"rusty\""),
            ("ext/rusty/src/lib.rs", b"pub fn f() {}"),
            ("spec/fixtures/helper.c", b"int y;"),
            ("bin/rusty-tool", b"\x7fELF\x02\x01\x01"),
        ],
    );
    let (size_bytes, sha256) = digest_file(gem_file.path());
    let metadata = extract_gem_metadata(
        gem_file.path(),
        "rusty",
        "2.0.0",
        None,
        size_bytes,
        &sha256,
        None,
    )
    .await
    .expect("metadata extraction succeeds")
    .expect("metadata is present");

    assert_eq!(
        metadata.native_languages,
        ["Rust"],
        "spec/ C does not count"
    );
    assert!(metadata.has_native_extensions);
    assert!(metadata.has_embedded_binaries, "an ELF in bin/ is a binary");
}
