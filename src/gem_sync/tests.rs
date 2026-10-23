use super::*;

#[test]
fn parse_compact_index_targets_parses_platform_variants() {
    let targets = parse_compact_index_targets(
        "---\n1.0.0 ruby|abc123|deps\n1.0.0 x86_64-linux|def456|deps\n2.0.0 |ghi789|deps\n",
    );

    assert_eq!(
        targets,
        vec![
            GemSyncTarget::new("1.0.0", None),
            GemSyncTarget::new("1.0.0", Some("x86_64-linux")),
            GemSyncTarget::new("2.0.0", None),
        ]
    );
}

#[test]
fn filter_targets_keeps_all_variants_when_platform_not_requested() {
    let targets = vec![
        GemSyncTarget::new("1.0.0", None),
        GemSyncTarget::new("1.0.0", Some("x86_64-linux")),
        GemSyncTarget::new("1.0.0", Some("x86_64-linux")),
    ];

    assert_eq!(
        filter_targets(targets, None),
        vec![
            GemSyncTarget::new("1.0.0", None),
            GemSyncTarget::new("1.0.0", Some("x86_64-linux")),
        ]
    );
}

#[test]
fn filter_targets_matches_ruby_platform_to_default_artifact() {
    let targets = vec![
        GemSyncTarget::new("1.0.0", None),
        GemSyncTarget::new("1.0.0", Some("java")),
    ];

    assert_eq!(
        filter_targets(targets, Some("ruby")),
        vec![GemSyncTarget::new("1.0.0", None)]
    );
}

#[test]
fn build_url_preserves_base_path() {
    let base = Uri::parse("https://mirror.example.com/rubygems/").unwrap();
    let url = build_url(&base, "/info/rack");

    assert_eq!(
        url.as_str(),
        "https://mirror.example.com/rubygems/info/rack"
    );
}

/// harbor's rake rows: analysed by an older analyzer, cached, and skipped by
/// every later sync until `--reanalyze` re-extracts them.
#[tokio::test]
async fn reanalyze_refreshes_the_metadata_of_cached_versions() {
    use crate::test_support::{build_test_gem, digest_file, install_rustls_provider};
    use vein_adapter::{AssetKey, AssetKind, CacheBackendTrait};

    install_rustls_provider();
    let cache = vein_adapter::test_support::connect_test_backend()
        .await
        .unwrap();
    let storage = tempfile::tempdir().unwrap();
    // MOCKED: no upstream is contacted; the version is already cached.
    let config: Config = toml::from_str(&format!(
        "[storage]\npath = {:?}\n[upstream]\nurl = \"http://127.0.0.1:9\"\n",
        storage.path()
    ))
    .unwrap();

    let gem = build_test_gem(
        "---\nname: rakeish\nversion: 1.0.0\nauthors: Dev\nlicenses: []\n",
        &[("doc/example/a.c", b"int main;"), ("lib/rakeish.rb", b"")],
    );
    let relative = "gems/rakeish/rakeish-1.0.0.gem";
    let path = storage.path().join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::copy(gem.path(), &path).unwrap();
    let (size, sha) = digest_file(&path);
    let key = AssetKey {
        kind: AssetKind::Gem,
        name: "rakeish",
        version: "1.0.0",
        platform: None,
    };
    cache
        .insert_or_replace(&key, relative, &sha, size)
        .await
        .unwrap();
    let mut stale = extract_gem_metadata(&path, "rakeish", "1.0.0", None, size, &sha, None)
        .await
        .unwrap()
        .unwrap();
    stale.native_languages = vec!["C".to_string()];
    cache.upsert_metadata(&stale).await.unwrap();

    let languages = async || {
        cache
            .gem_metadata("rakeish", "1.0.0", Some("ruby"))
            .await
            .unwrap()
            .unwrap()
            .native_languages
    };

    run(&config, &cache, "rakeish", Some("1.0.0"), None, false)
        .await
        .unwrap();
    assert_eq!(
        languages().await,
        ["C"],
        "a plain sync skips cached versions"
    );

    run(&config, &cache, "rakeish", Some("1.0.0"), None, true)
        .await
        .unwrap();
    assert_eq!(
        languages().await,
        Vec::<String>::new(),
        "doc/example is not native"
    );
}
