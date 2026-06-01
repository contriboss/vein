//! Catalog controller for RubyGems listing and details.

use anyhow::Result;
use rama::futures::StreamExt;
use rama::http::StatusCode;
use rama::http::service::web::extract::{Path, Query, State};
use rama::http::service::web::response::{Html, IntoResponse, Sse};
use rama::http::sse::Event;
use rama::http::sse::server::{KeepAlive, KeepAliveStream};
use serde::Deserialize;
use serde_json::to_string_pretty;
use tokio::sync::mpsc;
use vein_adapter::GemMetadata;

use crate::state::{AdminResources, AdminState};
use crate::utils::receiver_stream;
use crate::views;

const DEFAULT_PLATFORM: &str = "ruby";
const PAGE_SIZE: i64 = 100;

#[derive(Debug, Deserialize, Default)]
pub struct CatalogQuery {
    #[serde(default)]
    page: Option<i64>,
    #[serde(default)]
    lang: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SearchSignals {
    #[serde(default)]
    search: String,
}

#[derive(Debug, Deserialize)]
pub struct GemPath {
    name: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct GemDetailQuery {
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    platform: Option<String>,
}

struct CatalogDetailSelection {
    versions: Vec<String>,
    selected_version: Option<String>,
    requested_platform: String,
}

impl CatalogQuery {
    fn page_number(&self) -> i64 {
        self.page.unwrap_or(1).max(1)
    }

    fn selected_language(&self) -> Option<String> {
        self.lang
            .as_deref()
            .map(str::trim)
            .filter(|lang| !lang.is_empty())
            .map(str::to_string)
    }
}

impl GemDetailQuery {
    fn requested_platform(&self) -> &str {
        self.platform.as_deref().unwrap_or(DEFAULT_PLATFORM)
    }

    fn select_version(&self, versions: &[String]) -> Option<String> {
        self.version
            .as_ref()
            .and_then(|requested| versions.iter().find(|version| *version == requested))
            .cloned()
            .or_else(|| versions.first().cloned())
    }
}

pub async fn list(
    State(state): State<AdminState>,
    Query(query): Query<CatalogQuery>,
) -> impl IntoResponse {
    let data = match load_catalog_list(&state.resources, &query).await {
        Ok(data) => data,
        Err(err) => return error_html(err),
    };

    match views::catalog::list(&state.tera, data) {
        Ok(html) => Html(html),
        Err(e) => Html(format!("<h1>Template Error: {}</h1>", e)),
    }
}

/// SSE search for live catalog filtering
pub async fn search(
    State(state): State<AdminState>,
    Query(signals): Query<SearchSignals>,
) -> impl IntoResponse {
    // Create a channel for the single event
    let (tx, rx) = mpsc::channel::<Event<String>>(1);

    // Spawn task to do the async search
    tokio::spawn({
        let resources = state.resources.clone();
        let search_term = signals.search.clone();
        async move {
            let html = match resources.catalog_search(&search_term, 50).await {
                Ok(results) => views::catalog::search_results_html(&results),
                Err(_) => "<ul id='gem-list' class='gem-list'></ul>".to_string(),
            };

            // Use datastar SSE format for live search - single event
            let event = Event::default()
                .try_with_event("datastar-patch-elements")
                .expect("valid event name")
                .with_data(format!("fragments {}", html));

            let _ = tx.send(event).await;
        }
    });

    // Convert receiver to stream
    let stream = receiver_stream(rx);

    Sse::new(KeepAliveStream::new(
        KeepAlive::new(),
        stream.map(Ok::<_, std::convert::Infallible>),
    ))
}

pub async fn detail(
    State(state): State<AdminState>,
    Path(params): Path<GemPath>,
    Query(query): Query<GemDetailQuery>,
) -> impl IntoResponse {
    let data = match load_catalog_detail(&state.resources, &params.name, &query).await {
        Ok(data) => data,
        Err(err) => return error_html(err),
    };

    match views::catalog::detail(&state.tera, data) {
        Ok(html) => Html(html),
        Err(e) => Html(format!("<h1>Template Error: {}</h1>", e)),
    }
}

pub async fn sbom(
    State(state): State<AdminState>,
    Path(params): Path<GemPath>,
    Query(query): Query<GemDetailQuery>,
) -> impl IntoResponse {
    let selection = match load_detail_selection(&state.resources, &params.name, &query).await {
        Ok(selection) => selection,
        Err(_) => return (StatusCode::NOT_FOUND, "Not Found").into_response(),
    };

    let Some(version) = selection.selected_version.as_deref() else {
        return (StatusCode::NOT_FOUND, "Version not found").into_response();
    };

    let metadata = match state
        .resources
        .gem_metadata(&params.name, version, Some(&selection.requested_platform))
        .await
    {
        Ok(Some(m)) => m,
        Ok(None) => return (StatusCode::NOT_FOUND, "Metadata not found").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Error").into_response(),
    };

    let Some(sbom) = metadata.sbom.as_ref() else {
        return (StatusCode::NOT_FOUND, "SBOM not found").into_response();
    };

    let json_body = to_string_pretty(sbom).unwrap_or_else(|_| sbom.to_string());

    let platform_slug = &metadata.platform;
    let filename = format!(
        "{}-{}-{}.sbom.json",
        sanitize_for_filename(&metadata.name),
        sanitize_for_filename(&metadata.version),
        sanitize_for_filename(platform_slug)
    );

    (
        StatusCode::OK,
        [
            ("content-type", "application/json; charset=utf-8"),
            (
                "content-disposition",
                &format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        json_body,
    )
        .into_response()
}

async fn load_catalog_list(
    resources: &AdminResources,
    query: &CatalogQuery,
) -> Result<views::catalog::CatalogListData> {
    let page = query.page_number();
    let offset = (page - 1) * PAGE_SIZE;
    let selected_language = query.selected_language();
    let (entries, total) =
        load_catalog_entries(resources, selected_language.as_deref(), offset).await?;

    Ok(views::catalog::CatalogListData {
        entries: entries
            .into_iter()
            .map(|name| views::catalog::CatalogEntry { name })
            .collect(),
        page,
        total_pages: total.div_ceil(PAGE_SIZE as u64).max(1) as i64,
        total_label: format_total_label(total, selected_language.as_deref()),
        selected_language,
        languages: Vec::new(),
    })
}

async fn load_catalog_entries(
    resources: &AdminResources,
    selected_language: Option<&str>,
    offset: i64,
) -> Result<(Vec<String>, u64)> {
    if let Some(language) = selected_language {
        let entries = resources
            .catalog_page_by_language(language, offset, PAGE_SIZE)
            .await?;
        let total = resources.catalog_total_by_language(language).await?;
        Ok((entries, total))
    } else {
        let entries = resources.catalog_page(offset, PAGE_SIZE).await?;
        let total = resources.catalog_total().await?;
        Ok((entries, total))
    }
}

async fn load_catalog_detail(
    resources: &AdminResources,
    name: &str,
    query: &GemDetailQuery,
) -> Result<views::catalog::GemDetailData> {
    let selection = load_detail_selection(resources, name, query).await?;
    let metadata = load_gem_metadata(
        resources,
        name,
        selection.selected_version.as_deref(),
        &selection.requested_platform,
    )
    .await?;
    let platform = metadata
        .as_ref()
        .map(|meta| meta.platform.clone())
        .unwrap_or_else(|| selection.requested_platform.clone());

    Ok(views::catalog::GemDetailData {
        name: name.to_string(),
        versions: selection.versions,
        selected_version: selection
            .selected_version
            .unwrap_or_else(|| "—".to_string()),
        platform,
        platform_query: query.platform.clone(),
        metadata: metadata.as_ref().map(views::catalog::GemMetadataView::from),
    })
}

async fn load_detail_selection(
    resources: &AdminResources,
    name: &str,
    query: &GemDetailQuery,
) -> Result<CatalogDetailSelection> {
    let versions = resources.gem_versions(name).await?;
    Ok(CatalogDetailSelection {
        selected_version: query.select_version(&versions),
        versions,
        requested_platform: query.requested_platform().to_string(),
    })
}

async fn load_gem_metadata(
    resources: &AdminResources,
    name: &str,
    version: Option<&str>,
    platform: &str,
) -> Result<Option<GemMetadata>> {
    match version {
        Some(version) => resources.gem_metadata(name, version, Some(platform)).await,
        None => Ok(None),
    }
}

fn format_total_label(total: u64, selected_language: Option<&str>) -> String {
    match selected_language {
        Some(language) => format!("{total} ({language} only)"),
        None => total.to_string(),
    }
}

fn error_html(err: impl std::fmt::Display) -> Html<String> {
    Html(format!("<h1>Error: {}</h1>", err))
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
