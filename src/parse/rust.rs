#![cfg(feature = "crates")]

//! Rust crate parser using tree-sitter.

use anyhow::Result;

/// Parse Rust source code.
pub fn parse(source: &[u8]) -> Result<tree_sitter::Tree> {
    super::parse_with(tree_sitter_rust::LANGUAGE, source, "Rust")
}
