//! Catalog controller for gem listing and details.

use rama::futures::StreamExt;
use rama::http::service::web::extract::{Path, Query, State};
use rama::http::service::web::response::{Html, IntoResponse, Sse};
use rama::http::sse::server::{KeepAlive, KeepAliveStream};
use rama::http::sse::Event;
use rama::http::StatusCode;
use serde::Deserialize;
use serde_json::to_string_pretty;
use tokio::sync::mpsc;

use crate::state::AdminState;
use crate::utils::receiver_stream;
use crate::views;

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

pub async fn list(
    State(state): State<AdminState>,
    Query(query): Query<CatalogQuery>,
) -> impl IntoResponse {
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
        let entries = match state
            .resources
            .catalog_page_by_language(lang, offset, PAGE_SIZE)
            .await
        {
            Ok(e) => e,
            Err(e) => return Html(format!("<h1>Error: {}</h1>", e)),
        };
        let total = match state.resources.catalog_total_by_language(lang).await {
            Ok(t) => t,
            Err(e) => return Html(format!("<h1>Error: {}</h1>", e)),
        };
        (entries, total)
    } else {
        let entries = match state.resources.catalog_page(offset, PAGE_SIZE).await {
            Ok(e) => e,
            Err(e) => return Html(format!("<h1>Error: {}</h1>", e)),
        };
        let total = match state.resources.catalog_total().await {
            Ok(t) => t,
            Err(e) => return Html(format!("<h1>Error: {}</h1>", e)),
        };
        (entries, total)
    };

    let total_pages = total.div_ceil(PAGE_SIZE as u64).max(1) as i64;

    let total_label = if let Some(lang) = selected_language.as_deref() {
        format!("{} ({} only)", total, lang)
    } else {
        total.to_string()
    };

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
        stream.map(|event| Ok::<_, std::convert::Infallible>(event)),
    ))
}

pub async fn detail(
    State(state): State<AdminState>,
    Path(params): Path<GemPath>,
    Query(query): Query<GemDetailQuery>,
) -> impl IntoResponse {
    let versions = match state.resources.gem_versions(&params.name).await {
        Ok(v) => v,
        Err(e) => return Html(format!("<h1>Error: {}</h1>", e)),
    };

    let selected_version = query
        .version
        .as_ref()
        .and_then(|requested| versions.iter().find(|version| *version == requested))
        .cloned()
        .or_else(|| versions.first().cloned());

    // Default to "ruby" platform when not specified
    let query_platform = query.platform.as_deref().unwrap_or("ruby");

    let metadata = if let Some(version) = selected_version.as_deref() {
        match state
            .resources
            .gem_metadata(&params.name, version, Some(query_platform))
            .await
        {
            Ok(m) => m,
            Err(e) => return Html(format!("<h1>Error: {}</h1>", e)),
        }
    } else {
        None
    };

    let platform = metadata
        .as_ref()
        .map(|m| m.platform.as_str())
        .unwrap_or(query_platform);

    let data = views::catalog::GemDetailData {
        name: params.name,
        versions,
        selected_version: selected_version.unwrap_or_else(|| "—".to_string()),
        platform: platform.to_string(),
        platform_query: query.platform.clone(),
        metadata: metadata.as_ref().map(|m| m.into()),
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
    let versions = match state.resources.gem_versions(&params.name).await {
        Ok(v) => v,
        Err(_) => return (StatusCode::NOT_FOUND, "Not Found").into_response(),
    };

    let selected_version = query
        .version
        .as_ref()
        .and_then(|requested| versions.iter().find(|version| *version == requested))
        .cloned()
        .or_else(|| versions.first().cloned());

    let Some(version) = selected_version else {
        return (StatusCode::NOT_FOUND, "Version not found").into_response();
    };

    let query_platform = query.platform.as_deref().unwrap_or("ruby");

    let metadata = match state
        .resources
        .gem_metadata(&params.name, &version, Some(query_platform))
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
