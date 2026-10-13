//! Crates.io index types matching cargo's sparse registry format
//!
//! Reference: <https://doc.rust-lang.org/cargo/reference/registry-index.html>

use serde::{Deserialize, Serialize};

/// Sparse index config.json format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexConfig {
    /// Download URL template
    /// Use `{crate}` and `{version}` placeholders
    pub dl: String,
    /// API base URL (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    /// Auth required for downloads
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_required: Option<bool>,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            dl: "https://static.crates.io/crates/{crate}/{crate}-{version}.crate".to_string(),
            api: Some("https://crates.io".to_string()),
            auth_required: None,
        }
    }
}

/// Storage path of the cached sparse-index file for crate `name`.
#[must_use]
pub fn index_storage_path(name: &str) -> Option<String> {
    index_path(name).map(|path| format!("crates_index/{path}"))
}

/// Compute the index path prefix for a crate name
///
/// Cargo uses this to locate crate index files:
/// - 1 char: `1/{name}`
/// - 2 chars: `2/{name}`
/// - 3 chars: `3/{first_char}/{name}`
/// - 4+ chars: `{first_two}/{next_two}/{name}`
///
/// Returns `None` unless `name` is a valid crates.io name (ASCII
/// alphanumerics, `-`, `_`; at most 64 characters). Callers use the result
/// as a storage path, so this is also the traversal guard.
pub fn index_path(name: &str) -> Option<String> {
    let valid = name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if !valid {
        return None;
    }
    let name_lower = name.to_ascii_lowercase();
    match name_lower.len() {
        0 => None,
        1 => Some(format!("1/{name_lower}")),
        2 => Some(format!("2/{name_lower}")),
        3 => Some(format!("3/{}/{}", &name_lower[..1], name_lower)),
        _ => Some(format!(
            "{}/{}/{}",
            &name_lower[..2],
            &name_lower[2..4],
            name_lower
        )),
    }
}

#[cfg(test)]
mod tests;
