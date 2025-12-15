#[cfg(test)]
mod tests {
    use anyhow::{Context, Result};
    use serde_json::json;
    use sqlx::SqlitePool;

    use crate::cache::{
        CacheBackend,
        sqlite::SqliteCacheBackend,
        types::{AssetKey, AssetKind, CachedAsset, DependencyKind, GemDependency, GemMetadata},
    };

    async fn setup_test_db() -> SqliteCacheBackend {
        let backend = SqliteCacheBackend::connect_memory()
            .await
            .expect("Failed to create test database");

        // Initialize test schema (schema is normally managed by migrations)
        init_test_schema(&backend.pool)
            .await
            .expect("Failed to initialize test schema");

        backend
    }

    async fn init_test_schema(pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS cached_assets (
                kind TEXT NOT NULL,
                name TEXT NOT NULL,
                version TEXT NOT NULL,
                platform TEXT,
                path TEXT NOT NULL,
                sha256 TEXT NOT NULL,
                size_bytes INTEGER NOT NULL,
                last_accessed TIMESTAMP DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
                PRIMARY KEY (kind, name, version, platform)
            )
            "#,
        )
        .execute(pool)
        .await
        .context("creating cached_assets table")?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS catalog_gems (
                name TEXT PRIMARY KEY,
                latest_version TEXT,
                synced_at TIMESTAMP DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
            )
            "#,
        )
        .execute(pool)
        .await
        .context("creating catalog_gems table")?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS catalog_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await
        .context("creating catalog_meta table")?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS gem_metadata (
                name TEXT NOT NULL,
                version TEXT NOT NULL,
                platform TEXT,
                summary TEXT,
                description TEXT,
                licenses TEXT,
                authors TEXT,
                emails TEXT,
                homepage TEXT,
                documentation_url TEXT,
                changelog_url TEXT,
                source_code_url TEXT,
                bug_tracker_url TEXT,
                wiki_url TEXT,
                funding_url TEXT,
                metadata_json TEXT,
                dependencies_json TEXT NOT NULL,
                executables_json TEXT,
                extensions_json TEXT,
                native_languages_json TEXT,
                has_native_extensions INTEGER NOT NULL,
                has_embedded_binaries INTEGER NOT NULL,
                required_ruby_version TEXT,
                required_rubygems_version TEXT,
                rubygems_version TEXT,
                specification_version INTEGER,
                built_at TEXT,
                size_bytes INTEGER,
                sha256 TEXT,
                sbom_json TEXT,
                PRIMARY KEY (name, version, platform)
            )
            "#,
        )
        .execute(pool)
        .await
        .context("creating gem_metadata table")?;

        Ok(())
    }

    #[tokio::test]
    async fn test_asset_kind_as_str() {
        assert_eq!(AssetKind::Gem.as_str(), "gem");
        assert_eq!(AssetKind::Spec.as_str(), "gemspec");
    }

    #[tokio::test]
    async fn test_asset_kind_equality() {
        assert_eq!(AssetKind::Gem, AssetKind::Gem);
        assert_eq!(AssetKind::Spec, AssetKind::Spec);
        assert_ne!(AssetKind::Gem, AssetKind::Spec);
    }

    #[tokio::test]
    async fn test_cache_backend_initialization() {
        let backend = setup_test_db().await;
        let stats = backend.stats().await;
        assert!(
            stats.is_ok(),
            "Should be able to query initialized database"
        );
    }

    #[tokio::test]
    async fn test_insert_and_get_gem() {
        let backend = setup_test_db().await;

        let key = AssetKey {
            kind: AssetKind::Gem,
            name: "rails",
            version: "7.0.0",
            platform: None,
        };

        backend
            .insert_or_replace(&key, "/cache/rails-7.0.0.gem", "abc123def456", 1_024_000)
            .await
            .expect("Insert should succeed");

        let asset = backend.get(&key).await.expect("Get should succeed");
        assert!(asset.is_some(), "Asset should be found");

        let asset = asset.unwrap();
        assert_eq!(asset.path, "/cache/rails-7.0.0.gem");
        assert_eq!(asset.sha256, "abc123def456");
        assert_eq!(asset.size_bytes, 1_024_000);
    }

    #[tokio::test]
    async fn test_insert_and_get_spec() {
        let backend = setup_test_db().await;

        let key = AssetKey {
            kind: AssetKind::Spec,
            name: "nokogiri",
            version: "1.13.0",
            platform: None,
        };

        backend
            .insert_or_replace(
                &key,
                "/cache/nokogiri-1.13.0.gemspec.rz",
                "spec123hash",
                2048,
            )
            .await
            .expect("Insert should succeed");

        let asset = backend.get(&key).await.expect("Get should succeed");
        assert!(asset.is_some(), "Spec should be found");

        let asset = asset.unwrap();
        assert_eq!(asset.path, "/cache/nokogiri-1.13.0.gemspec.rz");
        assert_eq!(asset.sha256, "spec123hash");
        assert_eq!(asset.size_bytes, 2048);
    }

    #[tokio::test]
    async fn test_insert_with_platform() {
        let backend = setup_test_db().await;

        let key = AssetKey {
            kind: AssetKind::Gem,
            name: "nokogiri",
            version: "1.13.0",
            platform: Some("x86_64-linux"),
        };

        backend
            .insert_or_replace(
                &key,
                "/cache/nokogiri-1.13.0-x86_64-linux.gem",
                "platform123",
                3_072_000,
            )
            .await
            .expect("Insert should succeed");

        let asset = backend.get(&key).await.expect("Get should succeed");
        assert!(asset.is_some(), "Platform-specific asset should be found");

        let asset = asset.unwrap();
        assert_eq!(asset.path, "/cache/nokogiri-1.13.0-x86_64-linux.gem");
    }

    #[tokio::test]
    async fn test_platform_separation() {
        let backend = setup_test_db().await;

        let key_no_platform = AssetKey {
            kind: AssetKind::Gem,
            name: "nokogiri",
            version: "1.13.0",
            platform: None,
        };
        backend
            .insert_or_replace(
                &key_no_platform,
                "/cache/nokogiri-1.13.0.gem",
                "hash1",
                1000,
            )
            .await
            .expect("Insert should succeed");

        let key_with_platform = AssetKey {
            kind: AssetKind::Gem,
            name: "nokogiri",
            version: "1.13.0",
            platform: Some("x86_64-linux"),
        };
        backend
            .insert_or_replace(
                &key_with_platform,
                "/cache/nokogiri-1.13.0-x86_64-linux.gem",
                "hash2",
                2000,
            )
            .await
            .expect("Insert should succeed");

        let asset_no_platform = backend.get(&key_no_platform).await.unwrap().unwrap();
        let asset_with_platform = backend.get(&key_with_platform).await.unwrap().unwrap();

        assert_eq!(asset_no_platform.sha256, "hash1");
        assert_eq!(asset_with_platform.sha256, "hash2");
        assert_ne!(asset_no_platform.path, asset_with_platform.path);
    }

    #[tokio::test]
    async fn test_insert_or_replace_updates_existing() {
        let backend = setup_test_db().await;

        let key = AssetKey {
            kind: AssetKind::Gem,
            name: "rails",
            version: "7.0.0",
            platform: Some("ruby"),
        };

        backend
            .insert_or_replace(&key, "/cache/rails-7.0.0.gem", "old_hash", 1000)
            .await
            .expect("Insert should succeed");

        backend
            .insert_or_replace(&key, "/cache/new/rails-7.0.0.gem", "new_hash", 2000)
            .await
            .expect("Replace should succeed");

        let asset = backend.get(&key).await.unwrap().unwrap();
        assert_eq!(asset.path, "/cache/new/rails-7.0.0.gem");
        assert_eq!(asset.sha256, "new_hash");
        assert_eq!(asset.size_bytes, 2000);
    }

    #[tokio::test]
    async fn test_get_nonexistent_asset() {
        let backend = setup_test_db().await;

        let key = AssetKey {
            kind: AssetKind::Gem,
            name: "nonexistent",
            version: "1.0.0",
            platform: None,
        };

        let result = backend.get(&key).await.expect("Get should not error");
        assert!(result.is_none(), "Nonexistent asset should return None");
    }

    #[tokio::test]
    async fn test_get_all_gems_empty() {
        let backend = setup_test_db().await;

        let gems = backend.get_all_gems().await.expect("Should succeed");
        assert_eq!(gems.len(), 0, "Should return empty list for empty database");
    }

    #[tokio::test]
    async fn test_get_all_gems_with_data() {
        let backend = setup_test_db().await;

        let gems_to_insert = vec![
            ("rails", "7.0.0"),
            ("rails", "7.1.0"),
            ("nokogiri", "1.13.0"),
            ("nokogiri", "1.14.0"),
        ];

        for (name, version) in &gems_to_insert {
            let key = AssetKey {
                kind: AssetKind::Gem,
                name,
                version,
                platform: None,
            };
            backend
                .insert_or_replace(&key, "/cache/test.gem", "hash", 1_000)
                .await
                .expect("Insert should succeed");
        }

        let gems = backend.get_all_gems().await.expect("Should succeed");
        assert_eq!(gems.len(), gems_to_insert.len());
    }

    #[tokio::test]
    async fn test_stats_on_empty_db() {
        let backend = setup_test_db().await;

        let stats = backend.stats().await.expect("Stats should succeed");
        assert_eq!(stats.total_assets, 0);
        assert_eq!(stats.gem_assets, 0);
        assert_eq!(stats.spec_assets, 0);
        assert_eq!(stats.unique_gems, 0);
        assert_eq!(stats.total_size_bytes, 0);
        assert!(stats.last_accessed.is_none());
    }

    #[tokio::test]
    async fn test_stats_with_data() {
        let backend = setup_test_db().await;

        let gems = vec![
            AssetKey {
                kind: AssetKind::Gem,
                name: "rails",
                version: "7.0.0",
                platform: None,
            },
            AssetKey {
                kind: AssetKind::Gem,
                name: "rails",
                version: "7.1.0",
                platform: None,
            },
            AssetKey {
                kind: AssetKind::Spec,
                name: "rails",
                version: "7.1.0",
                platform: None,
            },
        ];

        for key in &gems {
            backend
                .insert_or_replace(key, "/cache/test", "hash", 1_000)
                .await
                .expect("Insert should succeed");
        }

        let stats = backend.stats().await.expect("Stats should succeed");
        assert_eq!(stats.total_assets, 3);
        assert_eq!(stats.gem_assets, 2);
        assert_eq!(stats.spec_assets, 1);
        assert_eq!(stats.unique_gems, 1);
        assert_eq!(stats.total_size_bytes, 3_000);
        assert!(stats.last_accessed.is_some());
    }

    #[tokio::test]
    async fn test_negative_size_clamped_to_zero() {
        use crate::cache::models::CachedAssetRow;

        let row = CachedAssetRow {
            path: "/test".to_string(),
            sha256: "hash".to_string(),
            size_bytes: -1000,
            last_accessed: "2024-01-01T12:00:00Z".to_string(),
        };

        let asset: CachedAsset = row.into();
        assert_eq!(asset.size_bytes, 0, "Negative sizes should be clamped to 0");
    }

    #[tokio::test]
    async fn test_special_characters_in_names() {
        let backend = setup_test_db().await;

        let special_names = vec![
            "rails-api",
            "activerecord_6.1",
            "my.gem",
            "gem@version",
            "test_123-abc.xyz",
        ];

        for name in special_names {
            let key = AssetKey {
                kind: AssetKind::Gem,
                name,
                version: "1.0.0",
                platform: None,
            };

            backend
                .insert_or_replace(&key, "/test", "hash", 1000)
                .await
                .expect("Should handle special characters");

            let asset = backend.get(&key).await.unwrap();
            assert!(
                asset.is_some(),
                "Should retrieve gem with special characters"
            );
        }
    }

    #[tokio::test]
    async fn test_version_formats() {
        let backend = setup_test_db().await;

        let versions = vec![
            "1.0.0",
            "1.0.0.pre",
            "2.0.0.beta1",
            "3.0.0.rc.1",
            "2024.10.27",
        ];

        for version in versions {
            let key = AssetKey {
                kind: AssetKind::Gem,
                name: "test-gem",
                version,
                platform: None,
            };

            backend
                .insert_or_replace(&key, "/test", "hash", 1000)
                .await
                .expect("Should handle version formats");

            let asset = backend.get(&key).await.unwrap();
            assert!(
                asset.is_some(),
                "Should retrieve gem with version {version}"
            );
        }
    }

    fn sample_metadata() -> GemMetadata {
        GemMetadata {
            name: "rack".to_string(),
            version: "2.2.8".to_string(),
            platform: None,
            summary: Some("Rack middleware toolkit".to_string()),
            description: Some("Minimal HTTP glue for Ruby web apps".to_string()),
            licenses: vec!["MIT".to_string()],
            authors: vec!["Rack Core Team".to_string()],
            emails: vec!["rack@example.test".to_string()],
            homepage: Some("https://rack.github.io".to_string()),
            documentation_url: Some("https://docs.rack.test".to_string()),
            changelog_url: Some("https://changelog.rack.test".to_string()),
            source_code_url: Some("https://github.com/rack/rack".to_string()),
            bug_tracker_url: Some("https://bugs.rack.test".to_string()),
            wiki_url: None,
            funding_url: None,
            metadata: json!({ "announcement": "Rack 2.2.8 released" }),
            dependencies: vec![GemDependency {
                name: "rack-proxy".to_string(),
                requirement: ">= 0.7".to_string(),
                kind: DependencyKind::Runtime,
            }],
            executables: vec!["rackup".to_string()],
            extensions: Vec::new(),
            native_languages: Vec::new(),
            has_native_extensions: false,
            has_embedded_binaries: false,
            required_ruby_version: Some(">= 2.7.0".to_string()),
            required_rubygems_version: None,
            rubygems_version: Some("3.4.7".to_string()),
            specification_version: Some(4),
            built_at: Some("2024-11-15".to_string()),
            size_bytes: 42_000,
            sha256: "deadbeefcafebabefeedface0123456789abcdef0123456789abcdefabcd".to_string(),
            sbom: None,
        }
    }

    #[tokio::test]
    async fn test_upsert_gem_metadata_persists_row() {
        let backend = setup_test_db().await;
        let metadata = sample_metadata();

        backend
            .upsert_gem_metadata_record(&metadata)
            .await
            .expect("metadata insert succeeds");

        let fetched = backend
            .gem_metadata(
                &metadata.name,
                &metadata.version,
                metadata.platform.as_deref(),
            )
            .await
            .expect("metadata fetch succeeds");

        assert_eq!(fetched, Some(metadata.clone()));
    }

    #[tokio::test]
    async fn test_upsert_gem_metadata_updates_existing_row() {
        let backend = setup_test_db().await;
        let mut metadata = sample_metadata();

        backend
            .upsert_gem_metadata_record(&metadata)
            .await
            .expect("initial insert");

        metadata.summary = Some("Rack middleware toolkit (updated)".to_string());
        metadata.has_embedded_binaries = true;
        metadata.metadata = json!({ "announcement": "Rack 2.2.9 beta", "beta": true });
        metadata.dependencies.push(GemDependency {
            name: "rack-protection".to_string(),
            requirement: "~> 3.0".to_string(),
            kind: DependencyKind::Optional,
        });

        backend
            .upsert_gem_metadata_record(&metadata)
            .await
            .expect("update insert");

        let fetched = backend
            .gem_metadata(
                &metadata.name,
                &metadata.version,
                metadata.platform.as_deref(),
            )
            .await
            .expect("updated metadata fetch succeeds")
            .expect("metadata should exist");

        assert_eq!(fetched.summary, metadata.summary);
        assert!(fetched.has_embedded_binaries);
        assert_eq!(fetched.metadata, metadata.metadata);
        assert_eq!(fetched.dependencies, metadata.dependencies);
    }

    #[tokio::test]
    async fn test_gem_metadata_not_found() {
        let backend = setup_test_db().await;

        let result = backend
            .gem_metadata("nonexistent", "0.0.1", None)
            .await
            .expect("metadata lookup succeeds");

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn catalog_languages_and_filters() {
        let backend = setup_test_db().await;

        let mut rust_meta = sample_metadata();
        rust_meta.name = "oxidized".to_string();
        rust_meta.native_languages = vec!["Rust".to_string()];
        backend
            .upsert_gem_metadata_record(&rust_meta)
            .await
            .expect("store rust metadata");

        let mut c_meta = sample_metadata();
        c_meta.name = "ffi-tool".to_string();
        c_meta.native_languages = vec!["C".to_string(), "Native Binary".to_string()];
        backend
            .upsert_gem_metadata_record(&c_meta)
            .await
            .expect("store c metadata");

        let languages = backend.catalog_languages().await.expect("languages");
        assert_eq!(languages, vec!["C", "Native Binary", "Rust"]);

        let rust_page = backend
            .catalog_page_by_language("Rust", 0, 10)
            .await
            .expect("rust page");
        assert_eq!(rust_page, vec!["oxidized"]);
        let rust_total = backend
            .catalog_total_by_language("Rust")
            .await
            .expect("rust total");
        assert_eq!(rust_total, 1);

        let c_page = backend
            .catalog_page_by_language("C", 0, 10)
            .await
            .expect("c page");
        assert_eq!(c_page, vec!["ffi-tool"]);
        let c_total = backend
            .catalog_total_by_language("C")
            .await
            .expect("c total");
        assert_eq!(c_total, 1);
    }
}
