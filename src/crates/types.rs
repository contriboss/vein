//! Crates.io index types matching cargo's sparse registry format
//!
//! Reference: https://doc.rust-lang.org/cargo/reference/registry-index.html

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

/// Compute the index path prefix for a crate name
///
/// Cargo uses this to locate crate index files:
/// - 1 char: `1/{name}`
/// - 2 chars: `2/{name}`
/// - 3 chars: `3/{first_char}/{name}`
/// - 4+ chars: `{first_two}/{next_two}/{name}`
pub fn index_path(name: &str) -> String {
    let name_lower = name.to_lowercase();
    match name_lower.len() {
        0 => panic!("empty crate name"),
        1 => format!("1/{}", name_lower),
        2 => format!("2/{}", name_lower),
        3 => format!("3/{}/{}", &name_lower[..1], name_lower),
        _ => format!("{}/{}/{}", &name_lower[..2], &name_lower[2..4], name_lower),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_path() {
        assert_eq!(index_path("a"), "1/a");
        assert_eq!(index_path("ab"), "2/ab");
        assert_eq!(index_path("abc"), "3/a/abc");
        assert_eq!(index_path("abcd"), "ab/cd/abcd");
        assert_eq!(index_path("serde"), "se/rd/serde");
        assert_eq!(index_path("Tokio"), "to/ki/tokio");
    }
}
