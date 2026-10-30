#![cfg(feature = "crates")]

//! Rust crate parser using tree-sitter.

use anyhow::Result;
use tree_sitter::{Language, Parser};

unsafe extern "C" {
    // SAFETY: tree-sitter-rust exports this function as part of its C API
    fn tree_sitter_rust() -> Language;
}

/// Parse Rust source code.
pub fn parse(source: &[u8]) -> Result<tree_sitter::Tree> {
    let mut parser = Parser::new();
    // SAFETY: tree_sitter_rust is a safe, exported C function from the tree-sitter library
    let language = unsafe { tree_sitter_rust() };
    parser.set_language(&language)?;

    parser
        .parse(source, None)
        .ok_or_else(|| anyhow::anyhow!("Failed to parse Rust source"))
}
