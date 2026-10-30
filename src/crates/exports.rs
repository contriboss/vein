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

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Function => write!(f, "function"),
            Self::Struct => write!(f, "struct"),
            Self::Enum => write!(f, "enum"),
            Self::Trait => write!(f, "trait"),
            Self::Module => write!(f, "module"),
            Self::Type => write!(f, "type"),
            Self::Constant => write!(f, "constant"),
            Self::Macro => write!(f, "macro"),
            Self::ReExport => write!(f, "reexport"),
        }
    }
}

/// Extract public APIs from crate source.
#[cfg(feature = "crates")]
pub fn extract_exports(source: &[u8], crate_name: &str) -> Result<Vec<Export>> {
    let tree = parse::rust::parse(source)?;
    let mut exports = Vec::new();
    let root = tree.root_node();

    walk_node(root, source, crate_name, "", &mut exports);

    Ok(exports)
}

#[cfg(feature = "crates")]
fn walk_node(
    node: tree_sitter::Node,
    source: &[u8],
    crate_name: &str,
    current_path: &str,
    exports: &mut Vec<Export>,
) {
    if is_public_item(node, source)
        && let Some(export) = extract_symbol(node, source, crate_name, current_path)
    {
        exports.push(export);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let new_path = if child.kind() == "mod_item" {
            if let Some(name) = get_node_text(child, source, "name") {
                if current_path.is_empty() {
                    name
                } else {
                    format!("{current_path}::{name}")
                }
            } else {
                current_path.to_string()
            }
        } else {
            current_path.to_string()
        };

        walk_node(child, source, crate_name, &new_path, exports);
    }
}

#[cfg(feature = "crates")]
fn is_public_item(node: tree_sitter::Node, source: &[u8]) -> bool {
    let kind = node.kind();
    if !matches!(
        kind,
        "function_item"
            | "struct_item"
            | "enum_item"
            | "trait_item"
            | "mod_item"
            | "type_alias"
            | "const_item"
            | "use_declaration"
    ) {
        return false;
    }

    node.child_by_field_name("visibility").is_some_and(|v| {
        let text = get_node_text_raw(v, source);
        text.starts_with("pub")
    })
}

#[cfg(feature = "crates")]
fn extract_symbol(
    node: tree_sitter::Node,
    source: &[u8],
    crate_name: &str,
    module_path: &str,
) -> Option<Export> {
    let kind_str = node.kind();
    let name =
        get_node_text(node, source, "name").or_else(|| get_first_identifier(node, source))?;

    let kind = match kind_str {
        "function_item" => SymbolKind::Function,
        "struct_item" => SymbolKind::Struct,
        "enum_item" => SymbolKind::Enum,
        "trait_item" => SymbolKind::Trait,
        "mod_item" => SymbolKind::Module,
        "type_alias" => SymbolKind::Type,
        "const_item" => SymbolKind::Constant,
        "use_declaration" => SymbolKind::ReExport,
        _ => return None,
    };

    let path = if module_path.is_empty() {
        format!("{crate_name}::{name}")
    } else {
        format!("{crate_name}::{module_path}::{name}")
    };

    Some(Export {
        name,
        kind,
        path,
        docs: extract_docs(node, source),
    })
}

#[cfg(feature = "crates")]
fn extract_docs(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let prev = node.prev_sibling()?;
    if matches!(prev.kind(), "line_comment" | "block_comment") {
        Some(get_node_text_raw(prev, source))
    } else {
        None
    }
}

#[cfg(feature = "crates")]
fn get_node_text(node: tree_sitter::Node, source: &[u8], field: &str) -> Option<String> {
    node.child_by_field_name(field)
        .map(|n| get_node_text_raw(n, source))
}

#[cfg(feature = "crates")]
fn get_first_identifier(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|n| n.kind() == "identifier")
        .map(|n| get_node_text_raw(n, source))
}

#[cfg(feature = "crates")]
fn get_node_text_raw(node: tree_sitter::Node, source: &[u8]) -> String {
    let start = node.start_byte();
    let end = node.end_byte();
    String::from_utf8_lossy(&source[start..end]).to_string()
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
