use super::*;

/// A token file holding `text`, and its directory's guard.
fn token_file(text: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("githits.token");
    std::fs::write(&path, text).unwrap();
    (dir, path)
}

#[test]
fn the_token_is_the_first_line_of_its_file() {
    // The environment wins; with it set here, the file is not read.
    if std::env::var(GITHITS_TOKEN_ENV).is_ok() {
        return;
    }
    let (_dir, path) = token_file("ghi-abc123\n# a note\n");
    let config = GitHitsConfig {
        token_file: Some(path),
        ..GitHitsConfig::default()
    };
    assert_eq!(config.token().unwrap(), "ghi-abc123");

    let (_dir, empty) = token_file("\n");
    let err = GitHitsConfig {
        token_file: Some(empty),
        ..GitHitsConfig::default()
    }
    .token()
    .unwrap_err();
    assert!(err.to_string().contains("holds no GitHits token"), "{err}");

    let err = GitHitsConfig::default().token().unwrap_err();
    assert!(err.to_string().contains(GITHITS_TOKEN_ENV), "{err}");
}

#[test]
fn only_the_listed_tools_are_offered() {
    let parsed: CodeContextConfig = toml::from_str(
        r#"
        provider = "githits"
        tools = ["search", "read"]
        "#,
    )
    .unwrap();
    assert_eq!(parsed.provider, Some(Provider::GitHits));
    assert_eq!(parsed.githits.url, "https://mcp.githits.com");
    assert!(parsed.offers("search") && parsed.offers("read"));
    assert!(!parsed.offers("research"));

    let every: CodeContextConfig = toml::from_str("provider = \"githits\"").unwrap();
    assert!(every.offers("research"), "no list: all of them");
    assert!(
        !CodeContextConfig::default().offers("search"),
        "no provider: none"
    );
}

#[test]
fn a_token_file_may_live_in_the_home_directory() {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let home = PathBuf::from(home);
    assert_eq!(
        home_relative(std::path::Path::new("~/.config/vein/githits.token")),
        home.join(".config/vein/githits.token")
    );
    assert_eq!(
        home_relative(std::path::Path::new("/usr/local/etc/vein/githits.token")),
        PathBuf::from("/usr/local/etc/vein/githits.token")
    );
}
