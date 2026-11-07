//! Extract and index public APIs from Rust crates using `tree-sitter`.

use anyhow::{Context, Result};
use std::io::Read;
use vein_adapter::PackageExport;

#[cfg(feature = "crates")]
use crate::parse;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// The public API of a `.crate` archive, read from its `src/lib.rs`; none
/// for a crate without one (a binary).
pub fn from_crate(tarball: &[u8], name: &str, version: &str) -> Result<Vec<PackageExport>> {
    match extract_lib_rs_from_crate(tarball, name, version).context("reading the crate")? {
        Some(lib_rs) => extract_exports(&lib_rs, name),
        None => Ok(Vec::new()),
    }
}

/// Extract public APIs from crate source.
#[cfg(feature = "crates")]
pub fn extract_exports(source: &[u8], crate_name: &str) -> Result<Vec<PackageExport>> {
    let tree = parse::rust::parse(source)?;
    let mut exports = Vec::new();
    let root = tree.root_node();

    // Paths are as Rust code names them: `state-machines` is `state_machines`.
    let crate_name = crate_name.replace('-', "_");
    walk_node(root, source, &crate_name, "", &mut exports);

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
    exports: &mut Vec<PackageExport>,
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
) -> Option<PackageExport> {
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

    Some(PackageExport {
        symbol: name,
        kind: kind.to_string(),
        path,
        docs: extract_docs(node, source),
    })
}

#[cfg(feature = "crates")]
/// The summary of `node`'s doc comment: the `///` lines (or `/** */`
/// block) right above it, past any attributes. Plain comments are not docs.
fn extract_docs(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    // Collected bottom-up, then put back in reading order.
    let mut lines: Vec<String> = Vec::new();
    let mut previous = node.prev_sibling();
    while let Some(sibling) = previous {
        let text = get_node_text_raw(sibling, source);
        match sibling.kind() {
            "attribute_item" => {}
            "line_comment" if text.starts_with("///") && !text.starts_with("////") => {
                lines.push(text.trim_start_matches("///").trim().to_owned());
            }
            "block_comment" if text.starts_with("/**") && !text.starts_with("/***") => {
                let body = text.trim_start_matches("/**").trim_end_matches("*/");
                lines.extend(
                    body.lines()
                        .rev()
                        .map(|line| line.trim().trim_start_matches('*').trim().to_owned()),
                );
                break;
            }
            _ => break,
        }
        previous = sibling.prev_sibling();
    }
    lines.reverse();
    crate::exports::summary(lines.iter().map(String::as_str))
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
        let source = include_bytes!("../../tests/fixtures/exports/my-example/src/lib.rs");

        let exports = extract_exports(source, "my-example").expect("parse failed");
        let found: Vec<(&str, &str)> = exports
            .iter()
            .map(|export| (export.path.as_str(), export.kind.as_str()))
            .collect();

        assert_eq!(
            found,
            [
                ("my_example::hello", "function"),
                ("my_example::World", "struct"),
                ("my_example::Mood", "enum"),
                ("my_example::utils", "module"),
                ("my_example::utils::helper", "function"),
                ("my_example::a", "module"),
                ("my_example::a::new", "function"),
                ("my_example::b", "module"),
                ("my_example::b::new", "function"),
                ("my_example::HashMap", "reexport"),
                ("my_example::Shared", "reexport"),
            ]
        );
        let docs: Vec<Option<&str>> = exports
            .iter()
            .take(3)
            .map(|export| export.docs.as_deref())
            .collect();
        assert_eq!(docs, [Some("Says hello."), None, Some("A block doc.")]);
    }
}
