#![cfg(feature = "gems")]

//! Ruby gem parser using tree-sitter.

use anyhow::Result;

/// Parse Ruby source code.
pub fn parse(source: &[u8]) -> Result<tree_sitter::Tree> {
    super::parse_with(tree_sitter_ruby::LANGUAGE, source, "Ruby")
}
