use axum::http::{StatusCode, header};
use loco_rs::prelude::*;
use serde::Deserialize;
use serde_json::to_string_pretty;
use std::sync::Arc;
use tera::Tera;

use super::resources;
use crate::views;

const PAGE_SIZE: i64 = 100;

pub fn routes() -> Routes {
    Routes::new()
        .prefix("catalog")
        .add("/", get(list))
        .add("/{name}", get(detail))
        .add("/{name}/sbom", get(sbom))
}

#[derive(Debug, Deserialize, Default)]
struct CatalogQuery {
    #[serde(default)]
    page: Option<i64>,
    #[serde(default)]
    lang: Option<String>,
    #[serde(default)]
    q: Option<String>,
}

#[debug_handler]
async fn list(
    State(ctx): State<AppContext>,
    Query(query): Query<CatalogQuery>,
) -> Result<Response> {
    let tera = ctx
        .shared_store
        .get::<Arc<Tera>>()
        .ok_or_else(|| Error::Message("Tera not initialized".to_string()))?;

    let resources = resources(&ctx)?;
    let page = query.page.unwrap_or(1).max(1);
    let offset = (page - 1) * PAGE_SIZE;

    let selected_language = query.lang.as_ref().and_then(|lang| {
        let trimmed = lang.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });

    let (entries, total) = if let Some(lang) = selected_language.as_deref() {
        let entries = resources
            .catalog_page_by_language(lang, offset, PAGE_SIZE)
            .await
            .map_err(|err| Error::Message(err.to_string()))?;
        let total = resources
            .catalog_total_by_language(lang)
            .await
            .map_err(|err| Error::Message(err.to_string()))?;
        (entries, total)
    } else {
        let entries = resources
            .catalog_page(offset, PAGE_SIZE)
            .await
            .map_err(|err| Error::Message(err.to_string()))?;
        let total = resources
            .catalog_total()
            .await
            .map_err(|err| Error::Message(err.to_string()))?;
        (entries, total)
    };
    let total_pages = total.div_ceil(PAGE_SIZE as u64).max(1) as i64;

    let total_label = if let Some(lang) = selected_language.as_deref() {
        format!("{} ({} only)", total, lang)
    } else {
        total.to_string()
    };

    // TODO: Fetch actual languages from the database once the feature is complete
    let languages: Vec<String> = Vec::new();

    let data = views::catalog::CatalogListData {
        entries: entries
            .into_iter()
            .map(|name| views::catalog::CatalogEntry { name })
            .collect(),
        page,
        total_pages,
        total_label,
        selected_language,
        languages,
    };

    views::catalog::list(&tera, data)
}

#[derive(Debug, Deserialize)]
struct GemPath {
    name: String,
}

#[derive(Debug, Deserialize, Default)]
struct GemDetailQuery {
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    platform: Option<String>,
}

#[debug_handler]
async fn detail(
    State(ctx): State<AppContext>,
    Path(params): Path<GemPath>,
    Query(query): Query<GemDetailQuery>,
) -> Result<Response> {
    let tera = ctx
        .shared_store
        .get::<Arc<Tera>>()
        .ok_or_else(|| Error::Message("Tera not initialized".to_string()))?;

    let resources = resources(&ctx)?;
    let versions = resources
        .gem_versions(&params.name)
        .await
        .map_err(|err| Error::Message(err.to_string()))?;

    let selected_version = query
        .version
        .as_ref()
        .and_then(|requested| versions.iter().find(|version| *version == requested))
        .cloned()
        .or_else(|| versions.first().cloned());

    let metadata = if let Some(version) = selected_version.as_deref() {
        resources
            .gem_metadata(&params.name, version, query.platform.as_deref())
            .await
            .map_err(|err| Error::Message(err.to_string()))?
    } else {
        None
    };

    let platform = query
        .platform
        .as_deref()
        .or_else(|| metadata.as_ref().and_then(|m| m.platform.as_deref()))
        .unwrap_or("ruby");

    let data = views::catalog::GemDetailData {
        name: params.name,
        versions,
        selected_version: selected_version.unwrap_or_else(|| "—".to_string()),
        platform: platform.to_string(),
        platform_query: query.platform.clone(),
        metadata: metadata.as_ref().map(|m| m.into()),
    };

    views::catalog::detail(&tera, data)
}

#[debug_handler]
async fn sbom(
    State(ctx): State<AppContext>,
    Path(params): Path<GemPath>,
    Query(query): Query<GemDetailQuery>,
) -> Result<Response> {
    let resources = resources(&ctx)?;
    let versions = resources
        .gem_versions(&params.name)
        .await
        .map_err(|err| Error::Message(err.to_string()))?;

    let selected_version = query
        .version
        .as_ref()
        .and_then(|requested| versions.iter().find(|version| *version == requested))
        .cloned()
        .or_else(|| versions.first().cloned());

    let Some(version) = selected_version else {
        return Err(Error::NotFound);
    };

    let metadata = resources
        .gem_metadata(&params.name, &version, query.platform.as_deref())
        .await
        .map_err(|err| Error::Message(err.to_string()))?;

    let Some(meta) = metadata else {
        return Err(Error::NotFound);
    };

    let Some(sbom) = meta.sbom.as_ref() else {
        return Err(Error::NotFound);
    };

    let json_body = to_string_pretty(sbom).unwrap_or_else(|_| sbom.to_string());

    let platform_slug = meta.platform.as_deref().unwrap_or("ruby");
    let filename = format!(
        "{}-{}-{}.sbom.json",
        sanitize_for_filename(&meta.name),
        sanitize_for_filename(&meta.version),
        sanitize_for_filename(platform_slug)
    );

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        )
        .body(json_body.into())
        .map_err(|err| Error::Message(err.to_string()))?;

    Ok(response)
}

fn sanitize_for_filename(input: &str) -> String {
    let sanitized: String = input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "artifact".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::{Path, Query, State},
        http::StatusCode,
    };
    use http_body_util::BodyExt;
    use loco_rs::{
        app::{AppContext, SharedStore},
        cache,
        config::{self as loco_config},
        controller::middleware,
        environment::Environment,
        storage::{self, Storage},
    };
    use sea_orm::DatabaseConnection;
    use serde_json::json;
    use std::sync::Arc;
    use vein::config::Config as VeinConfig;
    use vein_adapter::{
        AssetKey, AssetKind, CacheBackendKind, DependencyKind, GemDependency, GemMetadata,
        SqliteCacheBackend,
    };

    use crate::{ruby::RubyStatus, state::AdminResources};

    #[tokio::test]
    async fn sbom_endpoint_serves_cyclonedx_json() {
        let cache = Arc::new(build_in_memory_cache().await);
        let config = Arc::new(VeinConfig::default());
        let ruby_status = Arc::new(RubyStatus::default());
        let resources = AdminResources::new(config, cache, ruby_status);

        let ctx = build_app_context();
        ctx.shared_store.insert(resources);

        let response = sbom(
            State(ctx),
            Path(GemPath {
                name: "rack".to_string(),
            }),
            Query(GemDetailQuery {
                version: Some("2.2.8".to_string()),
                platform: None,
            }),
        )
        .await
        .expect("sbom route should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers();
        let content_type = headers
            .get(header::CONTENT_TYPE)
            .expect("content-type header present")
            .to_str()
            .expect("header is valid utf-8");
        assert!(
            content_type.starts_with("application/json"),
            "content-type should be JSON, got {content_type}"
        );

        let body_bytes = response
            .into_body()
            .collect()
            .await
            .expect("read sbom body")
            .to_bytes();
        let body_str =
            String::from_utf8(body_bytes.to_vec()).expect("sbom body should be utf-8 string");
        assert!(
            body_str.contains("\"CycloneDX\""),
            "SBOM body should contain CycloneDX marker"
        );
    }

    async fn build_in_memory_cache() -> CacheBackendKind {
        let backend = SqliteCacheBackend::connect_memory()
            .await
            .expect("create in-memory cache");

        let cache: CacheBackendKind = backend.into();

        cache
            .insert_or_replace(
                &AssetKey {
                    kind: AssetKind::Gem,
                    name: "rack",
                    version: "2.2.7",
                    platform: None,
                },
                "/cache/rack-2.2.7.gem",
                "sha-old",
                2048,
            )
            .await
            .expect("insert first version");

        cache
            .insert_or_replace(
                &AssetKey {
                    kind: AssetKind::Gem,
                    name: "rack",
                    version: "2.2.8",
                    platform: None,
                },
                "/cache/rack-2.2.8.gem",
                "sha-new",
                4096,
            )
            .await
            .expect("insert latest version");

        cache
            .upsert_metadata(&sample_metadata(
                "2.2.7",
                "Rack 2.2.7 summary",
                "rack-mini-profiler",
            ))
            .await
            .expect("store metadata 2.2.7");

        cache
            .upsert_metadata(&sample_metadata(
                "2.2.8",
                "Rack 2.2.8 summary",
                "rack-proxy",
            ))
            .await
            .expect("store metadata 2.2.8");

        cache
    }

    fn sample_metadata(version: &str, summary: &str, dependency: &str) -> GemMetadata {
        GemMetadata {
            name: "rack".to_string(),
            version: version.to_string(),
            platform: None,
            summary: Some(summary.to_string()),
            description: Some(format!("{summary} description")),
            licenses: vec!["MIT".to_string()],
            authors: vec!["Rack Core Team".to_string()],
            emails: vec!["rack@example.test".to_string()],
            homepage: Some("https://rack.test".to_string()),
            documentation_url: Some("https://docs.rack.test".to_string()),
            changelog_url: None,
            source_code_url: Some("https://github.com/rack/rack".to_string()),
            bug_tracker_url: None,
            wiki_url: None,
            funding_url: None,
            metadata: json!({ "release": version }),
            dependencies: vec![GemDependency {
                name: dependency.to_string(),
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
            sha256: format!("sha256-{version}"),
            sbom: Some(json!({
                "bomFormat": "CycloneDX",
                "specVersion": "1.5",
                "version": 1,
                "metadata": {
                    "component": {
                        "name": "rack",
                        "version": version,
                        "purl": format!("pkg:gem/rack@{version}")
                    }
                }
            })),
        }
    }

    fn build_app_context() -> AppContext {
        // Mock database connection for tests
        let mock_db = DatabaseConnection::default();

        AppContext {
            environment: Environment::Test,
            db: mock_db,
            queue_provider: None,
            config: loco_config::Config {
                logger: loco_config::Logger {
                    enable: false,
                    pretty_backtrace: false,
                    level: loco_rs::logger::LogLevel::Off,
                    format: loco_rs::logger::Format::Json,
                    override_filter: None,
                    file_appender: None,
                },
                server: loco_config::Server {
                    binding: "127.0.0.1".to_string(),
                    port: 0,
                    host: "127.0.0.1".to_string(),
                    ident: None,
                    middlewares: middleware::Config::default(),
                },
                cache: loco_config::CacheConfig::Null,
                queue: None,
                auth: None,
                workers: loco_config::Workers {
                    mode: loco_config::WorkerMode::ForegroundBlocking,
                },
                mailer: None,
                initializers: None,
                settings: None,
                scheduler: None,
            },
            mailer: None,
            storage: Storage::single(storage::drivers::null::new()).into(),
            cache: cache::Cache::new(cache::drivers::null::new()).into(),
            shared_store: Arc::new(SharedStore::default()),
        }
    }
}
