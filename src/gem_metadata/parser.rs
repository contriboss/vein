use std::{collections::BTreeSet, fs::File, io::Read, path::Path};

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use serde_yaml::{self, Mapping, Value as YamlValue};
use tar::Archive;
use tracing::warn;
use vein_adapter::{DependencyKind, GemDependency, GemMetadata};

use crate::gem_metadata::{
    analyzer::{analyze_data_tar, detect_language_from_path},
    sbom::generate_cyclonedx_sbom,
};

pub fn parse_gem_metadata(
    path: &Path,
    name: &str,
    version: &str,
    platform: Option<String>,
    size_bytes: u64,
    sha256: &str,
    existing_sbom: Option<serde_json::Value>,
) -> Result<Option<GemMetadata>> {
    let file = File::open(path).with_context(|| format!("opening gem at {}", path.display()))?;
    let mut archive = Archive::new(file);

    let mut metadata_yaml: Option<String> = None;
    let mut has_native_extensions = false;
    let mut has_embedded_binaries = false;
    let mut detected_languages = BTreeSet::new();

    for entry in archive.entries().context("reading gem archive entries")? {
        let entry = entry.context("accessing gem archive entry")?;
        let path = entry.path().context("reading entry path")?.into_owned();
        match path.as_os_str().to_string_lossy().as_ref() {
            "metadata.gz" => {
                let mut decoder = GzDecoder::new(entry);
                let mut buffer = String::new();
                decoder
                    .read_to_string(&mut buffer)
                    .context("decompressing gem metadata")?;
                metadata_yaml = Some(buffer);
            }
            "data.tar.gz" => {
                let mut decoder = GzDecoder::new(entry);
                let (native, vendor, languages) = analyze_data_tar(&mut decoder)?;
                has_native_extensions |= native;
                has_embedded_binaries |= vendor;
                detected_languages.extend(languages);
            }
            _ => {}
        }
    }

    let metadata_yaml = match metadata_yaml {
        Some(value) => value,
        None => return Ok(None),
    };

    let yaml_value: YamlValue =
        serde_yaml::from_str(&metadata_yaml).context("parsing gem metadata YAML")?;
    let mapping = match unwrap_tag(&yaml_value) {
        YamlValue::Mapping(map) => map,
        _ => return Ok(None),
    };

    let authors = extract_string_list(mapping_lookup(mapping, "authors"));
    let licenses = extract_string_list(mapping_lookup(mapping, "licenses"));
    let emails = extract_string_list(mapping_lookup(mapping, "email"));

    let summary = extract_string(mapping_lookup(mapping, "summary"));
    let description = extract_string(mapping_lookup(mapping, "description"));
    let homepage = extract_string(mapping_lookup(mapping, "homepage"));
    let spec_platform = extract_string(mapping_lookup(mapping, "platform"));

    let metadata_json = match mapping_lookup(mapping, "metadata") {
        Some(value) => match serde_yaml::from_value::<serde_json::Value>(unwrap_tag(value).clone())
        {
            Ok(v) => v,
            Err(err) => {
                warn!(error = %err, "failed to decode gem metadata map");
                serde_json::Value::Null
            }
        },
        None => serde_json::Value::Null,
    };

    let documentation_url = lookup_metadata_url(&metadata_json, "documentation_uri");
    let changelog_url = lookup_metadata_url(&metadata_json, "changelog_uri");
    let source_code_url = lookup_metadata_url(&metadata_json, "source_code_uri");
    let bug_tracker_url = lookup_metadata_url(&metadata_json, "bug_tracker_uri");
    let wiki_url = lookup_metadata_url(&metadata_json, "wiki_uri");
    let funding_url = lookup_metadata_url(&metadata_json, "funding_uri");

    let dependencies = parse_dependencies(mapping_lookup(mapping, "dependencies"));
    let executables = extract_string_list(mapping_lookup(mapping, "executables"));
    let extensions = extract_string_list(mapping_lookup(mapping, "extensions"));

    for extension_path in &extensions {
        if let Some(lang) = detect_language_from_path(extension_path) {
            detected_languages.insert(lang.to_string());
        }
    }

    let required_ruby_version = parse_requirement(mapping_lookup(mapping, "required_ruby_version"));
    let required_rubygems_version =
        parse_requirement(mapping_lookup(mapping, "required_rubygems_version"));
    let rubygems_version = extract_string(mapping_lookup(mapping, "rubygems_version"));
    let specification_version = extract_integer(mapping_lookup(mapping, "specification_version"));
    let built_at = extract_string(mapping_lookup(mapping, "date"));

    let mut effective_has_native_extensions = has_native_extensions;
    if !extensions.is_empty() {
        effective_has_native_extensions = true;
    }
    if spec_platform.as_deref().is_some_and(|plat| plat != "ruby") {
        effective_has_native_extensions = true;
    }

    let native_languages: Vec<String> = detected_languages.into_iter().collect();

    let mut metadata = GemMetadata {
        name: name.to_string(),
        version: version.to_string(),
        platform: spec_platform.or(platform),
        summary,
        description,
        licenses,
        authors,
        emails,
        homepage,
        documentation_url,
        changelog_url,
        source_code_url,
        bug_tracker_url,
        wiki_url,
        funding_url,
        metadata: metadata_json,
        dependencies,
        executables,
        extensions,
        native_languages,
        has_native_extensions: effective_has_native_extensions,
        has_embedded_binaries,
        required_ruby_version,
        required_rubygems_version,
        rubygems_version,
        specification_version,
        built_at,
        size_bytes,
        sha256: sha256.to_string(),
        sbom: None,
    };

    match generate_cyclonedx_sbom(&metadata, existing_sbom) {
        Ok(sbom) => {
            metadata.sbom = sbom;
        }
        Err(err) => {
            warn!(error = %err, "failed to build CycloneDX SBOM");
        }
    }

    Ok(Some(metadata))
}

pub fn parse_dependencies(value: Option<&YamlValue>) -> Vec<GemDependency> {
    let sequence = match value.map(unwrap_tag) {
        Some(YamlValue::Sequence(items)) => items,
        _ => return Vec::new(),
    };

    sequence.iter().filter_map(parse_dependency).collect()
}

fn parse_dependency(value: &YamlValue) -> Option<GemDependency> {
    let mapping = match unwrap_tag(value) {
        YamlValue::Mapping(map) => map,
        _ => return None,
    };

    let name = extract_string(mapping_lookup(mapping, "name"))?;
    let kind_raw =
        extract_string(mapping_lookup(mapping, "type")).unwrap_or_else(|| "runtime".to_string());
    let kind_key = kind_raw.trim_start_matches(':').to_ascii_lowercase();
    let kind = kind_key.parse().unwrap_or(DependencyKind::Unknown);

    let requirement_value = mapping_lookup(mapping, "requirement")
        .or_else(|| mapping_lookup(mapping, "version_requirements"));
    let requirement = parse_requirement(requirement_value).unwrap_or_else(|| ">= 0".to_string());

    Some(GemDependency {
        name,
        requirement,
        kind,
    })
}

pub fn parse_requirement(value: Option<&YamlValue>) -> Option<String> {
    let mapping = match value.map(unwrap_tag) {
        Some(YamlValue::Mapping(map)) => map,
        _ => return None,
    };
    let requirements = mapping_lookup(mapping, "requirements")?;
    let sequence = match unwrap_tag(requirements) {
        YamlValue::Sequence(seq) => seq,
        _ => return None,
    };

    let mut parts = Vec::new();
    for requirement in sequence {
        let elements = match unwrap_tag(requirement) {
            YamlValue::Sequence(seq) => seq,
            _ => continue,
        };
        if elements.is_empty() {
            continue;
        }
        let operator = elements
            .first()
            .and_then(|value| extract_string(Some(value)))
            .unwrap_or_default();
        let version = elements
            .get(1)
            .and_then(|value| extract_string(Some(value)))
            .unwrap_or_default();
        if !operator.is_empty() && !version.is_empty() {
            parts.push(format!("{operator} {version}"));
        } else if !version.is_empty() {
            parts.push(version);
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

pub fn extract_integer(value: Option<&YamlValue>) -> Option<i64> {
    match value.map(unwrap_tag) {
        Some(YamlValue::Number(num)) => num.as_i64(),
        _ => None,
    }
}

pub fn mapping_lookup<'a>(mapping: &'a Mapping, key: &str) -> Option<&'a YamlValue> {
    let key_value = YamlValue::String(key.to_string());
    mapping.get(&key_value)
}

pub fn extract_string_list(value: Option<&YamlValue>) -> Vec<String> {
    match value.map(unwrap_tag) {
        Some(YamlValue::Sequence(items)) => items
            .iter()
            .filter_map(|item| extract_string(Some(item)))
            .collect(),
        Some(other) => extract_string(Some(other)).into_iter().collect(),
        None => Vec::new(),
    }
}

pub fn extract_string(value: Option<&YamlValue>) -> Option<String> {
    let value = unwrap_tag_opt(value)?;
    match value {
        YamlValue::String(s) => Some(s.clone()),
        YamlValue::Null => None,
        YamlValue::Sequence(seq) if !seq.is_empty() => extract_string(Some(&seq[0])),
        YamlValue::Mapping(map) => {
            if let Some(inner) = mapping_lookup(map, "version") {
                extract_string(Some(inner))
            } else if let Some(inner) = mapping_lookup(map, "name") {
                extract_string(Some(inner))
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn lookup_metadata_url(metadata: &serde_json::Value, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
}

pub fn unwrap_tag(value: &YamlValue) -> &YamlValue {
    let mut current = value;
    while let YamlValue::Tagged(tagged) = current {
        current = &tagged.value;
    }
    current
}

pub fn unwrap_tag_opt(value: Option<&YamlValue>) -> Option<&YamlValue> {
    value.map(unwrap_tag)
}
