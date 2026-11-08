use super::*;

fn settings(write_token_file: Option<PathBuf>) -> McpSettings {
    McpSettings {
        writes: true,
        write_token_file,
        ..McpSettings::default()
    }
}

/// The environment wins; else the file's first line; else no write token.
#[test]
fn the_write_token_comes_from_the_environment_or_its_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("mcp-write.token");
    std::fs::write(&file, "from-file\nignored\n").unwrap();

    let both = settings(Some(file));
    assert_eq!(
        both.write_token(Some(" from-env ".into()))
            .unwrap()
            .as_deref(),
        Some("from-env")
    );
    assert_eq!(
        both.write_token(Some("  ".into())).unwrap().as_deref(),
        Some("from-file")
    );
    assert_eq!(settings(None).write_token(None).unwrap(), None);

    let missing = settings(Some(dir.path().join("absent.token")));
    assert!(missing.write_token(None).is_err());
}
