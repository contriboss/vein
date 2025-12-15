use std::{collections::BTreeSet, convert::TryFrom};

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde_json::Value as JsonValue;

use super::models::DbGemMetadataRow;
use super::types::{GemDependency, GemMetadata};

pub struct PreparedMetadataStrings {
    pub licenses_json: String,
    pub authors_json: String,
    pub emails_json: String,
    pub dependencies_json: String,
    pub executables_json: String,
    pub extensions_json: String,
    pub native_languages_json: String,
    pub metadata_json: Option<String>,
    pub size_bytes: i64,
    pub sbom_json: Option<String>,
}

pub fn prepare_metadata_strings(metadata: &GemMetadata) -> Result<PreparedMetadataStrings> {
    let licenses_json =
        serde_json::to_string(&metadata.licenses).context("serializing licenses")?;
    let authors_json = serde_json::to_string(&metadata.authors).context("serializing authors")?;
    let emails_json = serde_json::to_string(&metadata.emails).context("serializing emails")?;
    let dependencies_json =
        serde_json::to_string(&metadata.dependencies).context("serializing dependencies")?;
    let executables_json =
        serde_json::to_string(&metadata.executables).context("serializing executables")?;
    let extensions_json =
        serde_json::to_string(&metadata.extensions).context("serializing extensions")?;
    let native_languages_json = serde_json::to_string(&metadata.native_languages)
        .context("serializing native language list")?;
    let metadata_json = if metadata.metadata.is_null() {
        None
    } else {
        Some(serde_json::to_string(&metadata.metadata).context("serializing metadata json")?)
    };
    let sbom_json = match &metadata.sbom {
        Some(value) => Some(serde_json::to_string(value).context("serializing sbom json")?),
        None => None,
    };
    let size_bytes = i64::try_from(metadata.size_bytes).unwrap_or(i64::MAX);

    Ok(PreparedMetadataStrings {
        licenses_json,
        authors_json,
        emails_json,
        dependencies_json,
        executables_json,
        extensions_json,
        native_languages_json,
        metadata_json,
        size_bytes,
        sbom_json,
    })
}

pub fn parse_language_rows(rows: Vec<Option<String>>) -> Result<Vec<String>> {
    let mut languages = BTreeSet::new();
    for row in rows.into_iter().flatten() {
        let items: Vec<String> =
            serde_json::from_str(&row).context("parsing native language list from database")?;
        languages.extend(items);
    }
    Ok(languages.into_iter().collect())
}

pub fn parse_json_array<T>(field: &str, raw: Option<String>) -> Result<Vec<T>>
where
    T: DeserializeOwned,
{
    match raw {
        Some(value) => {
            serde_json::from_str(&value).with_context(|| format!("parsing {field} json array"))
        }
        None => Ok(Vec::new()),
    }
}

pub fn parse_required_json_array<T>(field: &str, raw: String) -> Result<Vec<T>>
where
    T: DeserializeOwned,
{
    parse_json_array(field, Some(raw))
}

pub fn parse_json_value(field: &str, raw: Option<String>) -> Result<JsonValue> {
    match raw {
        Some(value) => {
            serde_json::from_str(&value).with_context(|| format!("parsing {field} json value"))
        }
        None => Ok(JsonValue::Null),
    }
}

pub fn hydrate_metadata_row(row: DbGemMetadataRow) -> Result<GemMetadata> {
    let licenses: Vec<String> = parse_required_json_array("licenses", row.licenses)?;
    let authors: Vec<String> = parse_required_json_array("authors", row.authors)?;
    let emails: Vec<String> = parse_required_json_array("emails", row.emails)?;
    let dependencies: Vec<GemDependency> =
        parse_required_json_array("dependencies", row.dependencies_json)?;
    let executables: Vec<String> = parse_json_array("executables", row.executables_json)?;
    let extensions: Vec<String> = parse_json_array("extensions", row.extensions_json)?;
    let native_languages: Vec<String> =
        parse_json_array("native_languages", row.native_languages_json)?;
    let metadata = parse_json_value("metadata", row.metadata_json)?;
    let sbom = match row.sbom_json {
        Some(raw) => {
            let value: JsonValue = serde_json::from_str(&raw).context("parsing sbom json value")?;
            (!value.is_null()).then_some(value)
        }
        None => None,
    };

    Ok(GemMetadata {
        name: row.name,
        version: row.version,
        platform: row.platform,
        summary: row.summary,
        description: row.description,
        licenses,
        authors,
        emails,
        homepage: row.homepage,
        documentation_url: row.documentation_url,
        changelog_url: row.changelog_url,
        source_code_url: row.source_code_url,
        bug_tracker_url: row.bug_tracker_url,
        wiki_url: row.wiki_url,
        funding_url: row.funding_url,
        metadata,
        dependencies,
        executables,
        extensions,
        native_languages,
        has_native_extensions: row.has_native_extensions,
        has_embedded_binaries: row.has_embedded_binaries,
        required_ruby_version: row.required_ruby_version,
        required_rubygems_version: row.required_rubygems_version,
        rubygems_version: row.rubygems_version,
        specification_version: row.specification_version,
        built_at: row.built_at,
        size_bytes: row.size_bytes.max(0) as u64,
        sha256: row.sha256,
        sbom,
    })
}
