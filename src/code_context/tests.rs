use super::*;

#[test]
fn no_provider_is_no_tools() {
    assert!(
        from_config(&CodeContextConfig::default())
            .unwrap()
            .is_none()
    );
}

#[cfg(feature = "githits")]
#[test]
fn githits_needs_its_token() {
    if std::env::var(crate::config::code_context::GITHITS_TOKEN_ENV).is_ok() {
        return;
    }
    let mut config = CodeContextConfig {
        provider: Some(Configured::GitHits),
        ..CodeContextConfig::default()
    };
    let err = from_config(&config).err().unwrap();
    assert!(err.to_string().contains("API token"), "{err}");

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("githits.token");
    std::fs::write(&file, "ghi-abc123\n").unwrap();
    config.githits.token_file = Some(file);
    let provider = from_config(&config).unwrap().unwrap();
    assert_eq!(provider.name(), "GitHits");
}
