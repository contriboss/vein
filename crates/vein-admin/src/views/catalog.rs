//! Catalog view helpers.

use serde::Serialize;
use tera::{Context, Tera};
use vein::util::format_bytes;
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

impl CatalogListData {
    fn context(&self) -> Context {
        let mut context = Context::new();
        context.insert("current_page", "catalog");
        context.insert("entries", &self.entries);
        context.insert("page", &self.page);
        context.insert("total_pages", &self.total_pages);
        context.insert("total_label", &self.total_label);
        context.insert("selected_language", &self.selected_language);
        context.insert("languages", &self.languages);
        context
    }
}

impl GemDetailData {
    fn context(&self) -> Context {
        let mut context = Context::new();
        context.insert("current_page", "catalog");
        context.insert("name", &self.name);
        context.insert("versions", &self.versions);
        context.insert("selected_version", &self.selected_version);
        context.insert("platform", &self.platform);
        context.insert("platform_query", &self.platform_query);
        context.insert("metadata", &self.metadata);
        context
    }
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
        let (sbom, sbom_json) = sbom_payload(meta);

        Self {
            summary: meta.summary.clone(),
            description: meta.description.clone(),
            description_paragraphs: description_paragraphs(meta.description.as_deref()),
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
            platform: Some(meta.platform.clone()),
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
            dependencies: dependency_views(meta),
            metadata_json: pretty_json(&meta.metadata),
            sbom,
            sbom_json,
            sbom_download_url: sbom_download_url(meta),
            purl: purl_for_gem(&meta.name, &meta.version, Some(&meta.platform)),
        }
    }
}

fn description_paragraphs(description: Option<&str>) -> Vec<String> {
    description
        .map(|text| {
            text.split("\n\n")
                .map(|segment| segment.replace('\n', "<br />"))
                .collect()
        })
        .unwrap_or_default()
}

fn dependency_views(meta: &GemMetadata) -> Vec<DependencyView> {
    meta.dependencies
        .iter()
        .map(|dep| DependencyView {
            name: dep.name.clone(),
            requirement: dep.requirement.clone(),
            kind: dependency_kind_label(&dep.kind).to_string(),
        })
        .collect()
}

fn dependency_kind_label(kind: &DependencyKind) -> &'static str {
    match kind {
        DependencyKind::Runtime => "runtime",
        DependencyKind::Development => "development",
        DependencyKind::Optional => "optional",
        DependencyKind::Unknown => "unknown",
    }
}

fn pretty_json(value: &serde_json::Value) -> Option<String> {
    (!value.is_null())
        .then(|| serde_json::to_string_pretty(value).ok())
        .flatten()
}

fn sbom_payload(meta: &GemMetadata) -> (bool, Option<String>) {
    match meta.sbom.as_ref() {
        Some(sbom) => (true, serde_json::to_string_pretty(sbom).ok()),
        None => (false, None),
    }
}

fn sbom_download_url(meta: &GemMetadata) -> Option<String> {
    meta.sbom.as_ref()?;

    let mut url = format!(
        "/catalog/{}/sbom?version={}",
        urlencoding::encode(&meta.name),
        urlencoding::encode(&meta.version)
    );
    if meta.platform != "ruby" {
        url.push_str("&platform=");
        url.push_str(&urlencoding::encode(&meta.platform));
    }
    Some(url)
}

pub fn list(tera: &Tera, data: CatalogListData) -> anyhow::Result<String> {
    Ok(tera.render("catalog/list.html", &data.context())?)
}

pub fn detail(tera: &Tera, data: GemDetailData) -> anyhow::Result<String> {
    Ok(tera.render("catalog/detail.html", &data.context())?)
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
