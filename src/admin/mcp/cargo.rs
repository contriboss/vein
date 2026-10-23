//! Cargo sparse-index files as the proxy caches them: one JSON object per
//! published version.
//!
//! Format: <https://doc.rust-lang.org/cargo/reference/registry-index.html>

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use vein_adapter::sort_newest_first;

/// One line of a sparse-index file.
#[derive(Deserialize)]
pub struct IndexEntry {
    pub vers: String,
    #[serde(default)]
    pub deps: Vec<IndexDependency>,
    pub cksum: String,
    #[serde(default)]
    pub features: BTreeMap<String, Vec<String>>,
    /// Features using newer syntax (`dep:`, `?`), kept apart for old cargo.
    #[serde(default)]
    pub features2: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub yanked: bool,
    #[serde(default)]
    pub links: Option<String>,
    #[serde(default)]
    pub rust_version: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct IndexDependency {
    pub name: String,
    pub req: String,
    /// `dev` or `build`; left out for `normal`.
    #[serde(
        default = "IndexDependency::normal",
        skip_serializing_if = "IndexDependency::is_normal"
    )]
    pub kind: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
    /// Left out when true, the default.
    #[serde(
        default = "IndexDependency::yes",
        skip_serializing_if = "IndexDependency::is_yes"
    )]
    pub default_features: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// The real crate name when the dependency is renamed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
}

impl IndexDependency {
    fn normal() -> String {
        "normal".to_string()
    }

    const fn yes() -> bool {
        true
    }

    fn is_normal(kind: &str) -> bool {
        kind == "normal"
    }

    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "serde's skip_serializing_if signature"
    )]
    const fn is_yes(value: &bool) -> bool {
        *value
    }
}

impl IndexEntry {
    /// `features` and `features2` together, as cargo sees them.
    pub fn all_features(&self) -> BTreeMap<String, Vec<String>> {
        let mut features = self.features.clone();
        features.extend(self.features2.clone());
        features
    }
}

/// Every version in an index file, newest first. Lines that do not parse
/// are skipped rather than failing the whole crate.
pub fn parse_index(bytes: &[u8]) -> Vec<IndexEntry> {
    let mut entries: Vec<IndexEntry> = String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    sort_newest_first(&mut entries, |entry| &entry.vers);
    entries
}
