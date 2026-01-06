use axum::response::{Html, IntoResponse, Response};
use loco_rs::prelude::*;
use serde::Serialize;
use tera::{Context, Tera};
use vein_adapter::{DependencyKind, GemMetadata};

#[derive(Debug, Serialize)]
pub struct CatalogListData {
    pub entries: Vec<CatalogEntry>,
    pub page: i64,
    pub total_pages: i64,
    pub total_label: String,
    pub selected_language: Option<String>,
    pub languages: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CatalogEntry {
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct GemDetailData {
    pub name: String,
    pub versions: Vec<String>,
    pub selected_version: String,
    pub platform: String,
    pub platform_query: Option<String>,
    pub metadata: Option<GemMetadataView>,
}

#[derive(Debug, Serialize)]
pub struct GemMetadataView {
    pub summary: Option<String>,
    pub description: Option<String>,
    pub description_paragraphs: Vec<String>,
    pub authors: Vec<String>,
    pub licenses: Vec<String>,
    pub emails: Vec<String>,
    pub homepage: Option<String>,
    pub documentation_url: Option<String>,
    pub changelog_url: Option<String>,
    pub source_code_url: Option<String>,
    pub bug_tracker_url: Option<String>,
    pub wiki_url: Option<String>,
    pub funding_url: Option<String>,
    pub platform: Option<String>,
    pub built_at: Option<String>,
    pub size_formatted: String,
    pub sha256: String,
    pub required_ruby_version: Option<String>,
    pub required_rubygems_version: Option<String>,
    pub has_native_extensions: bool,
    pub has_embedded_binaries: bool,
    pub executables: Vec<String>,
    pub extensions: Vec<String>,
    pub native_languages: Vec<String>,
    pub dependencies: Vec<DependencyView>,
    pub metadata_json: Option<String>,
    pub sbom: bool,
    pub sbom_json: Option<String>,
    pub sbom_download_url: Option<String>,
    pub purl: String,
}

#[derive(Debug, Serialize)]
pub struct DependencyView {
    pub name: String,
    pub requirement: String,
    pub kind: String,
}

fn purl_for_gem(name: &str, version: &str, platform: Option<&str>) -> String {
    let base = format!(
        "pkg:gem/{}@{}",
        urlencoding::encode(name),
        urlencoding::encode(version)
    );
    match platform {
        Some(p) if p != "ruby" => format!("{}?platform={}", base, urlencoding::encode(p)),
        _ => base,
    }
}

impl From<&GemMetadata> for GemMetadataView {
    fn from(meta: &GemMetadata) -> Self {
        let description_paragraphs = meta
            .description
            .as_ref()
            .map(|desc| {
                desc.split("\n\n")
                    .map(|segment| segment.replace('\n', "<br />"))
                    .collect()
            })
            .unwrap_or_default();

        let metadata_json = if !meta.metadata.is_null() {
            serde_json::to_string_pretty(&meta.metadata).ok()
        } else {
            None
        };

        let (sbom, sbom_json) = if let Some(sbom) = &meta.sbom {
            let json = serde_json::to_string_pretty(sbom).ok();
            (true, json)
        } else {
            (false, None)
        };

        let sbom_download_url = if meta.sbom.is_some() {
            let mut url = format!(
                "/catalog/{}/sbom?version={}",
                urlencoding::encode(&meta.name),
                urlencoding::encode(&meta.version)
            );
            if let Some(platform) = &meta.platform {
                url.push_str("&platform=");
                url.push_str(&urlencoding::encode(platform));
            }
            Some(url)
        } else {
            None
        };

        Self {
            summary: meta.summary.clone(),
            description: meta.description.clone(),
            description_paragraphs,
            authors: meta.authors.clone(),
            licenses: meta.licenses.clone(),
            emails: meta.emails.clone(),
            homepage: meta.homepage.clone(),
            documentation_url: meta.documentation_url.clone(),
            changelog_url: meta.changelog_url.clone(),
            source_code_url: meta.source_code_url.clone(),
            bug_tracker_url: meta.bug_tracker_url.clone(),
            wiki_url: meta.wiki_url.clone(),
            funding_url: meta.funding_url.clone(),
            platform: meta.platform.clone(),
            built_at: meta.built_at.clone(),
            size_formatted: format_bytes(meta.size_bytes),
            sha256: meta.sha256.clone(),
            required_ruby_version: meta.required_ruby_version.clone(),
            required_rubygems_version: meta.required_rubygems_version.clone(),
            has_native_extensions: meta.has_native_extensions,
            has_embedded_binaries: meta.has_embedded_binaries,
            executables: meta.executables.clone(),
            extensions: meta.extensions.clone(),
            native_languages: meta.native_languages.clone(),
            dependencies: meta
                .dependencies
                .iter()
                .map(|dep| DependencyView {
                    name: dep.name.clone(),
                    requirement: dep.requirement.clone(),
                    kind: match dep.kind {
                        DependencyKind::Runtime => "runtime",
                        DependencyKind::Development => "development",
                        DependencyKind::Optional => "optional",
                        DependencyKind::Unknown => "unknown",
                    }
                    .to_string(),
                })
                .collect(),
            metadata_json,
            sbom,
            sbom_json,
            sbom_download_url,
            purl: purl_for_gem(&meta.name, &meta.version, meta.platform.as_deref()),
        }
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "0 B".to_string();
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

pub fn list(tera: &Tera, data: CatalogListData) -> Result<Response> {
    let mut context = Context::new();
    context.insert("entries", &data.entries);
    context.insert("page", &data.page);
    context.insert("total_pages", &data.total_pages);
    context.insert("total_label", &data.total_label);
    context.insert("selected_language", &data.selected_language);
    context.insert("languages", &data.languages);

    let html = tera
        .render("catalog/list.html", &context)
        .map_err(|e| Error::Message(format!("Template error: {}", e)))?;

    Ok(Html(html).into_response())
}

pub fn detail(tera: &Tera, data: GemDetailData) -> Result<Response> {
    let mut context = Context::new();
    context.insert("name", &data.name);
    context.insert("versions", &data.versions);
    context.insert("selected_version", &data.selected_version);
    context.insert("platform", &data.platform);
    context.insert("platform_query", &data.platform_query);
    context.insert("metadata", &data.metadata);

    let html = tera
        .render("catalog/detail.html", &context)
        .map_err(|e| Error::Message(format!("Template error: {}", e)))?;

    Ok(Html(html).into_response())
}

pub fn search_results_html(results: &[String]) -> String {
    let items: String = results
        .iter()
        .map(|name| {
            let escaped = name
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            format!(
                r#"<li><a href="/catalog/{}" class="gem-link">{}</a></li>"#,
                urlencoding::encode(name),
                escaped
            )
        })
        .collect();

    format!(r#"<ul id="gem-list" class="gem-list">{}</ul>"#, items)
}
