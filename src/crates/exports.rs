//! Extract and index public APIs from Rust crates using `tree-sitter`.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::Read;

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

/// Records `node`'s public children under `module_path`, then descends: a
/// module's items sit under its name, the module itself under its parent's.
#[cfg(feature = "crates")]
fn walk_node(
    node: tree_sitter::Node,
    source: &[u8],
    crate_name: &str,
    module_path: &str,
    exports: &mut Vec<Export>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_public_item(child, source)
            && let Some(export) = extract_symbol(child, source, crate_name, module_path)
        {
            exports.push(export);
        }

        let child_path = match get_node_text(child, source, "name") {
            Some(name) if child.kind() == "mod_item" && module_path.is_empty() => name,
            Some(name) if child.kind() == "mod_item" => format!("{module_path}::{name}"),
            _ => module_path.to_string(),
        };
        walk_node(child, source, crate_name, &child_path, exports);
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

    // tree-sitter-rust has no `visibility` field: the modifier is a plain
    // child. Only a bare `pub` is the crate's API; `pub(crate)`,
    // `pub(super)` and `pub(in ..)` are not.
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == "visibility_modifier")
        .is_some_and(|modifier| get_node_text_raw(modifier, source) == "pub")
}

#[cfg(feature = "crates")]
fn extract_symbol(
    node: tree_sitter::Node,
    source: &[u8],
    crate_name: &str,
    module_path: &str,
) -> Option<Export> {
    let kind_str = node.kind();
    let name = if kind_str == "use_declaration" {
        reexported_name(node, source)?
    } else {
        get_node_text(node, source, "name")?
    };

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

/// The name a `pub use` brings into scope: `C` for `a::b::C`, `D` for
/// `a::C as D`. Lists and globs (`a::{B, C}`, `a::*`) name no single item.
#[cfg(feature = "crates")]
fn reexported_name(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let argument = node.child_by_field_name("argument")?;
    match argument.kind() {
        "identifier" => Some(get_node_text_raw(argument, source)),
        "scoped_identifier" => get_node_text(argument, source, "name"),
        "use_as_clause" => get_node_text(argument, source, "alias"),
        _ => None,
    }
}

#[cfg(feature = "crates")]
fn get_node_text_raw(node: tree_sitter::Node, source: &[u8]) -> String {
    let start = node.start_byte();
    let end = node.end_byte();
    String::from_utf8_lossy(&source[start..end]).to_string()
}

/// Extract lib.rs from a .crate tarball.
#[cfg(feature = "crates")]
pub fn extract_lib_rs_from_crate(
    tarball_bytes: &[u8],
    crate_name: &str,
    version: &str,
) -> Result<Option<Vec<u8>>> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    let gz = GzDecoder::new(tarball_bytes);
    let mut archive = Archive::new(gz);
    let lib_rs_path = format!("{crate_name}-{version}/src/lib.rs");

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;

        if path.to_string_lossy() == lib_rs_path {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            return Ok(Some(bytes));
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_exports() {
        let source = br"
            pub fn hello() {}
            pub struct World { name: String }
            pub mod utils { pub fn helper() {} pub(crate) fn internal() {} }
            pub use std::collections::HashMap;
            pub use std::sync::Arc as Shared;
            pub(super) struct Hidden;
            fn private() {}
        ";

        let exports = extract_exports(source, "example").expect("parse failed");
        let found: Vec<(&str, SymbolKind)> = exports
            .iter()
            .map(|export| (export.path.as_str(), export.kind.clone()))
            .collect();

        assert_eq!(
            found,
            [
                ("example::hello", SymbolKind::Function),
                ("example::World", SymbolKind::Struct),
                ("example::utils", SymbolKind::Module),
                ("example::utils::helper", SymbolKind::Function),
                ("example::HashMap", SymbolKind::ReExport),
                ("example::Shared", SymbolKind::ReExport),
            ]
        );
    }
}
