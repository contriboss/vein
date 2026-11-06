//! Language-specific parsers for extracting package metadata.
//! Only compiled when the corresponding registry feature is enabled.

#[cfg(feature = "gems")]
pub mod ruby;

#[cfg(feature = "crates")]
pub mod rust;

#[cfg(feature = "npm")]
pub mod typescript;

/// Parses `source` with a grammar crate's `LANGUAGE`. Naming the constant is
/// what links the grammar's C parser into vein: a hand-written `extern "C"`
/// declaration leaves the crate unused, and the linker drops its library.
#[cfg(any(feature = "gems", feature = "crates", feature = "npm"))]
fn parse_with(
    language: impl Into<tree_sitter::Language>,
    source: &[u8],
    what: &str,
) -> anyhow::Result<tree_sitter::Tree> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language.into())?;
    parser
        .parse(source, None)
        .ok_or_else(|| anyhow::anyhow!("Failed to parse {what} source"))
}
