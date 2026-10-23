//! npm packuments (registry metadata documents) as the proxy caches them.
//!
//! Only the fields agents need are read; packuments of popular packages run
//! to megabytes, so nothing is copied wholesale.

use std::collections::BTreeMap;

use serde_json::{Map, Value};
use vein_adapter::sort_newest_first;

pub struct Packument(Map<String, Value>);

impl Packument {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        match serde_json::from_slice(bytes).ok()? {
            Value::Object(document) => Some(Self(document)),
            _ => None,
        }
    }

    pub fn text(&self, field: &str) -> Option<String> {
        text(self.0.get(field)?)
    }

    pub fn license(&self) -> Option<String> {
        license(self.0.get("license")?)
    }

    /// Repository URL, from either `"repository": "url"` or `{"url": ...}`.
    pub fn repository(&self) -> Option<String> {
        match self.0.get("repository")? {
            Value::Object(repository) => text(repository.get("url")?),
            other => text(other),
        }
    }

    pub fn dist_tags(&self) -> BTreeMap<String, String> {
        string_map(self.0.get("dist-tags"))
    }

    /// Published versions, newest first.
    pub fn versions(&self) -> Vec<&str> {
        let mut versions: Vec<&str> = self
            .0
            .get("versions")
            .and_then(Value::as_object)
            .map(|versions| versions.keys().map(String::as_str).collect())
            .unwrap_or_default();
        sort_newest_first(&mut versions, |version| version);
        versions
    }

    /// The manifest npm serves for `version`.
    pub fn manifest(&self, version: &str) -> Option<&Map<String, Value>> {
        self.0.get("versions")?.get(version)?.as_object()
    }

    /// Why the author deprecated `version`, if they did. An empty message is
    /// how `npm deprecate <pkg> ""` lifts a deprecation, so it is none.
    pub fn deprecation(&self, version: &str) -> Option<&str> {
        self.manifest(version)?
            .get("deprecated")?
            .as_str()
            .filter(|message| !message.is_empty())
    }
}

/// `DefinitelyTyped`'s `ts4.6`-style dist-tags: the newest types that still
/// support that TypeScript release, generated for every `@types` package.
pub fn is_typescript_tag(tag: &str) -> bool {
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    tag.strip_prefix("ts")
        .and_then(|release| release.split_once('.'))
        .is_some_and(|(major, minor)| digits(major) && digits(minor))
}

pub fn text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_string)
}

/// SPDX expression, from either `"license": "MIT"` or the legacy
/// `{"type": "MIT"}` form.
pub fn license(value: &Value) -> Option<String> {
    match value {
        Value::Object(license) => text(license.get("type")?),
        other => text(other),
    }
}

/// A `{name: "string"}` object such as `dependencies` or `engines`.
pub fn string_map(value: Option<&Value>) -> BTreeMap<String, String> {
    value
        .and_then(Value::as_object)
        .map(|object| {
            object
                .iter()
                .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// Valid npm name as far as storage paths are concerned: no leading `.`/`_`,
/// no empty or dot-only path segments, at most 214 characters.
pub fn is_valid_name(name: &str) -> bool {
    name.len() <= 214
        && !name.starts_with(['.', '_'])
        && name
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

#[cfg(test)]
mod tests;
