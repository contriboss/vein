//! Text package authors wrote, made safe to hand to an agent.
//!
//! Summaries, descriptions, licenses, author names, URLs and deprecation
//! messages are all set by whoever publishes a package, so any of them can
//! carry an instruction aimed at the agent reading it (indirect prompt
//! injection). Resources keep that text under an `author_supplied` key,
//! which the server instructions tell agents to treat as data, never as
//! instructions; this module strips what a person reviewing it could not
//! see and caps what an attack would need room for.

use serde::Serialize;
use serde_json::Value;

/// Longest author text kept, in characters, before `…(truncated)`.
pub const SUMMARY_LIMIT: usize = 300;
pub const DESCRIPTION_LIMIT: usize = 2_000;
pub const DEPRECATION_LIMIT: usize = 300;
/// A license, an author name, a dist-tag.
pub const SHORT_LIMIT: usize = 200;
pub const URL_LIMIT: usize = 500;
/// Authors and licenses listed at most.
pub const LIST_LIMIT: usize = 20;

const TRUNCATED: &str = "…(truncated)";

/// Characters that render as nothing (or reorder what renders) yet reach the
/// model: Unicode tag characters (U+E0000–E007F, "ASCII smuggling"),
/// zero-width and joiner characters, bidi embeddings, overrides and
/// isolates, word joiners and invisible operators, the BOM, interlinear
/// annotations, and variation selectors (used to hide bytes after a
/// visible character).
const fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{034F}'
            | '\u{061C}'
            | '\u{115F}'..='\u{1160}'
            | '\u{17B4}'..='\u{17B5}'
            | '\u{180B}'..='\u{180F}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{3164}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0000}'..='\u{E007F}'
            | '\u{E0100}'..='\u{E01EF}'
    )
}

/// Drops invisible and control characters (ANSI escapes lose their `ESC`)
/// but keeps line breaks and tabs, then caps the result at `limit`
/// characters. `None` when nothing visible is left.
pub fn clean(text: &str, limit: usize) -> Option<String> {
    let kept: String = text
        .chars()
        .filter(|&c| !is_invisible(c) && (!c.is_control() || c == '\n' || c == '\t'))
        .collect();
    let kept = kept.trim();
    if kept.is_empty() {
        return None;
    }
    if kept.chars().count() <= limit {
        return Some(kept.to_string());
    }
    let mut cut: String = kept.chars().take(limit).collect();
    cut.push_str(TRUNCATED);
    Some(cut)
}

/// [`clean`] for an optional field.
pub fn clean_opt(text: Option<String>, limit: usize) -> Option<String> {
    text.and_then(|text| clean(&text, limit))
}

/// Up to [`LIST_LIMIT`] cleaned entries, each at most [`SHORT_LIMIT`].
pub fn clean_list(items: Vec<String>) -> Vec<String> {
    items
        .into_iter()
        .filter_map(|item| clean(&item, SHORT_LIMIT))
        .take(LIST_LIMIT)
        .collect()
}

/// An `http(s)` URL with nothing hidden in it; anything else (`javascript:`,
/// `data:`, `file:`, a bare string) is dropped.
pub fn clean_url(url: Option<String>) -> Option<String> {
    let url = clean(&url?, URL_LIMIT)?;
    let scheme = url.split_once("://")?.0;
    let web = scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https");
    (web && !url.chars().any(char::is_whitespace)).then_some(url)
}

/// Every string in `value` cleaned in place, for documents vein passes
/// through whole (the `CycloneDX` SBOM carries author text in its fields).
pub fn clean_json(value: &mut Value) {
    match value {
        Value::String(text) => *text = clean(text, DESCRIPTION_LIMIT).unwrap_or_default(),
        Value::Array(items) => items.iter_mut().for_each(clean_json),
        Value::Object(map) => map.values_mut().for_each(clean_json),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// A gem's author text.
#[derive(Serialize, Default)]
pub struct GemText {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub licenses: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Left out when it is the homepage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_code_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changelog_url: Option<String>,
}

impl GemText {
    pub const fn is_empty(&self) -> bool {
        self.summary.is_none()
            && self.description.is_none()
            && self.licenses.is_empty()
            && self.authors.is_empty()
            && self.homepage.is_none()
            && self.source_code_url.is_none()
            && self.documentation_url.is_none()
            && self.changelog_url.is_none()
    }
}

/// An npm package's or version's author text.
#[derive(Serialize, Default)]
pub struct NpmText {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Left out when it is the homepage's repository.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// Why the author deprecated this version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
}

impl NpmText {
    pub const fn is_empty(&self) -> bool {
        self.description.is_none()
            && self.license.is_none()
            && self.homepage.is_none()
            && self.repository.is_none()
            && self.deprecated.is_none()
    }
}

#[cfg(test)]
mod tests;
