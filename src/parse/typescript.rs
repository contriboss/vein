#![cfg(feature = "npm")]

//! JavaScript/TypeScript parser using tree-sitter.

use anyhow::Result;

/// Parse TypeScript/JavaScript source code.
pub fn parse(source: &[u8]) -> Result<tree_sitter::Tree> {
    super::parse_with(
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        source,
        "TypeScript/JavaScript",
    )
}
