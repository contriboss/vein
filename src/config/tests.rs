use super::*;
use rama::http::Uri;
use std::io::Write;
use std::{fs, str::FromStr};
use tempfile::{tempdir, NamedTempFile};

// === DEFAULT VALUE TESTS ===

#[test]
fn test_default_config() {
    let config = Config::default();
    assert_eq!(config.server.host, "0.0.0.0");
    assert_eq!(config.server.port, 8346);
    assert_eq!(config.server.workers, num_cpus::get());
    assert!(config.upstream.is_none());
    assert_eq!(config.storage.path, PathBuf::from("./gems"));
    #[cfg(feature = "sqlite")]
    assert_eq!(config.database.path, PathBuf::from("./vein.db"));
    assert!(config.database.url.is_none());
    assert_eq!(config.logging.level, "info");
    assert!(!config.logging.json);
}

#[test]
fn test_default_server_config() {
    let server = ServerConfig::default();
    assert_eq!(server.host, "0.0.0.0");
    assert_eq!(server.port, 8346);
    assert_eq!(server.workers, num_cpus::get());
}

#[test]
fn test_default_upstream_config() {
    let upstream = UpstreamConfig::default();
    assert_eq!(upstream.url.to_string(), "https://rubygems.org/");
}

#[test]
fn test_default_storage_config() {
    let storage = StorageConfig::default();
    assert_eq!(storage.path, PathBuf::from("./gems"));
}

#[test]
fn test_default_database_config() {
    let db = DatabaseConfig::default();
    #[cfg(feature = "sqlite")]
    assert_eq!(db.path, PathBuf::from("./vein.db"));
    assert!(db.url.is_none());
}

#[test]
fn test_default_logging_config() {
    let logging = LoggingConfig::default();
    assert_eq!(logging.level, "info");
    assert!(!logging.json);
}

// === TOML PARSING TESTS ===

#[test]
fn test_parse_minimal_config() {
    let toml = r#"
        [server]
        host = "127.0.0.1"
        port = 8080
    "#;
    let config: Config = toml::from_str(toml).unwrap();
    assert_eq!(config.server.host, "127.0.0.1");
    assert_eq!(config.server.port, 8080);
    assert!(config.upstream.is_none());
}

#[test]
fn test_parse_full_config() {
    let toml = r#"
        [server]
        host = "0.0.0.0"
        port = 3000
        workers = 4

        [upstream]
        url = "https://example.com/"
        timeout_secs = 60
        connection_pool_size = 256

        [storage]
        path = "/var/lib/vein/gems"

        [database]
        path = "/var/lib/vein/db.sqlite"

        [logging]
        level = "debug"
        json = true
    "#;
    let config: Config = toml::from_str(toml).unwrap();

    assert_eq!(config.server.host, "0.0.0.0");
    assert_eq!(config.server.port, 3000);
    assert_eq!(config.server.workers, 4);

    let upstream = config.upstream.unwrap();
    assert_eq!(upstream.url.to_string(), "https://example.com/");

    assert_eq!(config.storage.path, PathBuf::from("/var/lib/vein/gems"));
    #[cfg(feature = "sqlite")]
    assert_eq!(
        config.database.path,
        PathBuf::from("/var/lib/vein/db.sqlite")
    );
    assert!(config.database.url.is_none());

    assert_eq!(config.logging.level, "debug");
    assert!(config.logging.json);
}

#[test]
fn test_parse_config_with_defaults() {
    let toml = r#"
        [server]
        port = 9000
    "#;
    let config: Config = toml::from_str(toml).unwrap();
    assert_eq!(config.server.host, "0.0.0.0");
    assert_eq!(config.server.port, 9000);
    assert_eq!(config.server.workers, num_cpus::get());
}

#[test]
fn test_parse_empty_config() {
    let toml = "";
    let config: Config = toml::from_str(toml).unwrap();
    assert_eq!(config.server.host, "0.0.0.0");
    assert_eq!(config.server.port, 8346);
    assert!(config.upstream.is_none());
}

// === CONFIG FILE LOADING TESTS ===

#[test]
fn test_load_config_from_existing_file() {
    let temp_dir = tempdir().unwrap();
    let config_path = temp_dir.path().join("vein.toml");

    let toml_content = r#"
        [server]
        host = "127.0.0.1"
        port = 4000

        [storage]
        path = "my-gems"
    "#;

    fs::write(&config_path, toml_content).unwrap();

    let config = Config::load(Some(config_path.clone())).unwrap();
    assert_eq!(config.server.host, "127.0.0.1");
    assert_eq!(config.server.port, 4000);

    // Check path normalization
    let expected_storage = temp_dir.path().join("my-gems");
    assert_eq!(config.storage.path, expected_storage);
}

#[test]
fn test_load_config_nonexistent_file_uses_defaults() {
    let temp_dir = tempdir().unwrap();
    let nonexistent = temp_dir.path().join("nonexistent.toml");

    let config = Config::load(Some(nonexistent)).unwrap();
    assert_eq!(config.server.host, "0.0.0.0");
    assert_eq!(config.server.port, 8346);
}

#[test]
fn test_load_config_no_path_provided() {
    // If vein.toml doesn't exist in current dir, should use defaults
    let cwd = std::env::current_dir().unwrap();
    let tmp = tempdir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();

    let result = Config::load(None);
    assert!(result.is_ok());
    let config = result.unwrap();
    assert_eq!(config.server.port, 8346);

    // restore
    std::env::set_current_dir(cwd).unwrap();
}

#[test]
fn test_load_config_invalid_toml() {
    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(b"invalid { toml content").unwrap();
    temp_file.flush().unwrap();

    let result = Config::load(Some(temp_file.path().to_path_buf()));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("invalid config"));
}

#[test]
fn test_load_config_unreadable_file() {
    let result = Config::load(Some(PathBuf::from("/nonexistent/path/config.toml")));
    // Should use defaults when file doesn't exist
    assert!(result.is_ok());
}

// === PATH NORMALIZATION TESTS ===

#[test]
fn test_storage_path_normalization_relative() {
    let mut storage = StorageConfig {
        path: PathBuf::from("./my-gems"),
    };
    let base = Path::new("/var/lib/vein");
    storage.normalize_paths(base);
    assert_eq!(storage.path, PathBuf::from("/var/lib/vein/./my-gems"));
}

#[test]
fn test_storage_path_normalization_absolute() {
    let mut storage = StorageConfig {
        path: PathBuf::from("/absolute/path/gems"),
    };
    let base = Path::new("/var/lib/vein");
    storage.normalize_paths(base);
    assert_eq!(storage.path, PathBuf::from("/absolute/path/gems"));
}

#[test]
#[cfg(feature = "sqlite")]
fn test_database_path_normalization_relative() {
    let mut db = DatabaseConfig {
        path: PathBuf::from("vein.db"),
        ..DatabaseConfig::default()
    };
    let base = Path::new("/var/lib/vein");
    db.normalize_paths(base);
    assert_eq!(db.path, PathBuf::from("/var/lib/vein/vein.db"));
}

#[test]
#[cfg(feature = "sqlite")]
fn test_database_path_normalization_absolute() {
    let mut db = DatabaseConfig {
        path: PathBuf::from("/absolute/path/db.sqlite"),
        ..DatabaseConfig::default()
    };
    let base = Path::new("/var/lib/vein");
    db.normalize_paths(base);
    assert_eq!(db.path, PathBuf::from("/absolute/path/db.sqlite"));
}

#[test]
#[cfg(feature = "sqlite")]
fn test_database_backend_sqlite_default() {
    let backend = DatabaseConfig::default().backend().unwrap();
    assert_eq!(backend.path, PathBuf::from("./vein.db"));
}

#[test]
#[cfg(feature = "postgres")]
fn test_database_backend_postgres() {
    let db = DatabaseConfig {
        url: Some("postgres://user:pass@localhost/vein".to_string()),
        ..DatabaseConfig::default()
    };
    let backend = db.backend().unwrap();
    assert_eq!(backend.url, "postgres://user:pass@localhost/vein");
}

#[test]
fn test_database_backend_invalid_scheme() {
    let db = DatabaseConfig {
        url: Some("mysql://localhost/db".to_string()),
        ..DatabaseConfig::default()
    };
    assert!(db.backend().is_err());
}

#[test]
#[cfg(feature = "sqlite")]
fn test_database_backend_sqlite_url_absolute() {
    let db = DatabaseConfig {
        path: PathBuf::from("./vein.db"),
        url: Some("sqlite:///var/lib/vein/cache.db".to_string()),
        ..DatabaseConfig::default()
    };
    let backend = db.backend().unwrap();
    assert_eq!(backend.path, PathBuf::from("/var/lib/vein/cache.db"));
}

#[test]
#[cfg(feature = "sqlite")]
fn test_database_backend_sqlite_url_localhost() {
    let db = DatabaseConfig {
        path: PathBuf::from("./vein.db"),
        url: Some("sqlite://localhost/var/lib/vein/cache.db".to_string()),
        ..DatabaseConfig::default()
    };
    let backend = db.backend().unwrap();
    assert_eq!(backend.path, PathBuf::from("/var/lib/vein/cache.db"));
}

#[test]
#[cfg(feature = "sqlite")]
fn test_config_normalizes_paths_on_load() {
    let temp_dir = tempdir().unwrap();
    let config_path = temp_dir.path().join("test.toml");

    let toml = r#"
        [storage]
        path = "relative-gems"

        [database]
        path = "relative.db"
    "#;

    fs::write(&config_path, toml).unwrap();
    let config = Config::load(Some(config_path)).unwrap();

    assert_eq!(config.storage.path, temp_dir.path().join("relative-gems"));
    assert_eq!(config.database.path, temp_dir.path().join("relative.db"));
    assert!(config.database.url.is_none());
}

#[test]
#[cfg(feature = "sqlite")]
fn test_config_sqlite_url_sets_path() {
    let temp_dir = tempdir().unwrap();
    let config_path = temp_dir.path().join("vein.toml");

    let toml = r#"
        [database]
        url = "sqlite://./db/vein.sqlite"
    "#;

    fs::write(&config_path, toml).unwrap();
    let config = Config::load(Some(config_path)).unwrap();

    let expected = temp_dir.path().join("db/vein.sqlite");
    assert!(config.database.path.is_absolute());
    assert!(config.database.path.ends_with("db/vein.sqlite"));
    let backend = config.database.backend().unwrap();
    assert!(backend.path.is_absolute());
    assert!(backend.path.ends_with("db/vein.sqlite"));
    assert_eq!(backend.path, expected);
}

// === VALIDATION TESTS ===

#[test]
#[cfg(feature = "sqlite")]
fn test_validate_https_upstream() {
    let config = Config {
        upstream: Some(UpstreamConfig {
            url: Uri::from_str("https://rubygems.org/").unwrap(),
            ..UpstreamConfig::default()
        }),
        ..Default::default()
    };
    assert!(config.validate().is_ok());
}

#[test]
#[cfg(feature = "sqlite")]
fn test_validate_http_upstream() {
    let config = Config {
        upstream: Some(UpstreamConfig {
            url: Uri::from_str("http://localhost:8346/").unwrap(),
            ..UpstreamConfig::default()
        }),
        ..Default::default()
    };
    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_invalid_scheme() {
    let config = Config {
        upstream: Some(UpstreamConfig {
            url: Uri::from_str("ftp://example.com/").unwrap(),
            ..UpstreamConfig::default()
        }),
        ..Default::default()
    };
    let result = config.validate();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("unsupported upstream scheme"));
}

#[test]
#[cfg(feature = "sqlite")]
fn test_validate_no_upstream() {
    let config = Config {
        upstream: None,
        ..Default::default()
    };
    assert!(config.validate().is_ok());
}

// === URL DESERIALIZATION TESTS ===

#[test]
fn test_deserialize_valid_url() {
    let toml = r#"
        [upstream]
        url = "https://rubygems.org/"
    "#;
    let config: Config = toml::from_str(toml).unwrap();
    assert_eq!(
        config.upstream.unwrap().url.to_string(),
        "https://rubygems.org/"
    );
}

#[test]
fn test_deserialize_invalid_url() {
    let toml = r#"
        [upstream]
        url = "not a valid url"
    "#;
    let result: Result<Config, _> = toml::from_str(toml);
    assert!(result.is_err());
}

// === EDGE CASES ===

#[test]
fn test_config_clone() {
    let config = Config::default();
    let cloned = config.clone();
    assert_eq!(config.server.port, cloned.server.port);
}

#[test]
fn test_config_debug() {
    let config = Config::default();
    let debug_str = format!("{:?}", config);
    assert!(debug_str.contains("Config"));
}

#[test]
fn test_zero_workers() {
    let toml = r#"
        [server]
        workers = 0
    "#;
    let config: Config = toml::from_str(toml).unwrap();
    assert_eq!(config.server.workers, 0);
}

#[test]
fn test_various_log_levels() {
    for level in ["trace", "debug", "info", "warn", "error"] {
        let toml = format!(
            r#"
            [logging]
            level = "{}"
        "#,
            level
        );
        let config: Config = toml::from_str(&toml).unwrap();
        assert_eq!(config.logging.level, level);
    }
}

#[test]
fn test_json_logging_enabled() {
    let toml = r#"
        [logging]
        json = true
    "#;
    let config: Config = toml::from_str(toml).unwrap();
    assert!(config.logging.json);
}

#[test]
fn test_json_logging_disabled() {
    let toml = r#"
        [logging]
        json = false
    "#;
    let config: Config = toml::from_str(toml).unwrap();
    assert!(!config.logging.json);
}

#[test]
fn test_url_with_path() {
    let toml = r#"
        [upstream]
        url = "https://example.com/rubygems/"
    "#;
    let config: Config = toml::from_str(toml).unwrap();
    assert_eq!(
        config.upstream.unwrap().url.to_string(),
        "https://example.com/rubygems/"
    );
}

#[test]
fn test_url_with_port() {
    let toml = r#"
        [upstream]
        url = "https://example.com:8443/"
    "#;
    let config: Config = toml::from_str(toml).unwrap();
    assert_eq!(
        config.upstream.unwrap().url.to_string(),
        "https://example.com:8443/"
    );
}

#[test]
fn test_ipv4_host() {
    let toml = r#"
        [server]
        host = "192.168.1.100"
    "#;
    let config: Config = toml::from_str(toml).unwrap();
    assert_eq!(config.server.host, "192.168.1.100");
}

#[test]
fn test_ipv6_host() {
    let toml = r#"
        [server]
        host = "::1"
    "#;
    let config: Config = toml::from_str(toml).unwrap();
    assert_eq!(config.server.host, "::1");
}

#[test]
fn test_low_port_number() {
    let toml = r#"
        [server]
        port = 80
    "#;
    let config: Config = toml::from_str(toml).unwrap();
    assert_eq!(config.server.port, 80);
}

#[test]
fn test_high_port_number() {
    let toml = r#"
        [server]
        port = 65535
    "#;
    let config: Config = toml::from_str(toml).unwrap();
    assert_eq!(config.server.port, 65535);
}

// === INTEGRATION TESTS ===

#[test]
#[cfg(feature = "sqlite")]
fn test_full_workflow_load_validate() {
    let temp_dir = tempdir().unwrap();
    let config_path = temp_dir.path().join("vein.toml");

    let toml = r#"
        [server]
        host = "0.0.0.0"
        port = 8346
        workers = 4

        [upstream]
        url = "https://rubygems.org/"
        timeout_secs = 30

        [storage]
        path = "gems"

        [database]
        path = "vein.db"

        [logging]
        level = "info"
        json = false
    "#;

    fs::write(&config_path, toml).unwrap();

    let config = Config::load(Some(config_path.clone())).unwrap();
    assert!(config.validate().is_ok());

    assert_eq!(config.server.host, "0.0.0.0");
    assert_eq!(config.server.port, 8346);
    assert_eq!(
        config.upstream.as_ref().unwrap().url.to_string(),
        "https://rubygems.org/"
    );

    // Paths should be normalized relative to config file location
    assert_eq!(config.storage.path, temp_dir.path().join("gems"));
    #[cfg(feature = "sqlite")]
    assert_eq!(config.database.path, temp_dir.path().join("vein.db"));
}

#[test]
fn test_parent_directory_resolution() {
    let temp_dir = tempdir().unwrap();
    let subdir = temp_dir.path().join("configs");
    fs::create_dir(&subdir).unwrap();
    let config_path = subdir.join("vein.toml");

    let toml = r#"
        [storage]
        path = "gems"
    "#;

    fs::write(&config_path, toml).unwrap();
    let config = Config::load(Some(config_path)).unwrap();

    // Should be relative to the config file's parent directory
    assert_eq!(config.storage.path, subdir.join("gems"));
}
