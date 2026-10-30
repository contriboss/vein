//! Extract and index public APIs from Rust crates using `tree-sitter`.

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[cfg(feature = "crates")]
use crate::parse;

/// Public API symbol from a crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Export {
    pub name: String,
    pub kind: SymbolKind,
    pub path: String, // fully qualified path (e.g., "tokio::spawn")
    pub docs: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Module,
    Type,
    Constant,
    Macro,
    /// Re-export from another crate
    ReExport,
}

/// Extract public APIs from crate source.
#[cfg(feature = "crates")]
pub fn extract_exports(source: &[u8], _crate_name: &str) -> Result<Vec<Export>> {
    let tree = parse::rust::parse(source)?;
    let exports = Vec::new();

    // TODO: Walk tree and extract pub items
    // For now, placeholder implementation
    let _root = tree.root_node();

    Ok(exports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_exports() {
        let source = br"
            pub fn hello() {}
            pub struct World { name: String }
            pub mod utils { pub fn helper() {} }
            pub use std::collections::HashMap;
        ";

        let exports = extract_exports(source, "example").expect("parse failed");
        assert!(!exports.is_empty());
    }
}
