//! Where package documentation vein serves comes from, beyond the packages
//! themselves.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DocsConfig {
    /// Where crates' rustdoc JSON comes from: docs.rs, or a mirror of it.
    pub docs_rs: String,
}

impl Default for DocsConfig {
    fn default() -> Self {
        Self {
            docs_rs: "https://docs.rs".into(),
        }
    }
}
