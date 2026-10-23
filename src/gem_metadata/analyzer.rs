use std::{collections::BTreeSet, io::Read};

use anyhow::{Context, Result};
use tar::Archive;

const EMBEDDED_BINARY_DIR_PREFIXES: &[&str] = &["vendor/", "libexec/", "resources/"];

/// Where the native code a gem compiles lives. Sources elsewhere are docs,
/// examples, tests or fixtures (rake ships `doc/example/*.c`), not part of
/// what gets built; the gemspec's `extensions` are added by the parser.
const NATIVE_SOURCE_DIR: &str = "ext/";

/// Executable formats a file in `bin/` may start with: ELF, Mach-O (32/64,
/// both byte orders, fat), and PE (`MZ`).
const BINARY_MAGIC: &[&[u8]] = &[
    b"\x7fELF",
    &[0xfe, 0xed, 0xfa, 0xce],
    &[0xfe, 0xed, 0xfa, 0xcf],
    &[0xce, 0xfa, 0xed, 0xfe],
    &[0xcf, 0xfa, 0xed, 0xfe],
    &[0xca, 0xfe, 0xba, 0xbe],
    b"MZ",
];

/// Scans every file of a gem's `data.tar`: whether it builds native
/// extensions, whether it ships prebuilt binaries, and the languages of the
/// extensions it compiles.
pub fn analyze_data_tar<R: Read>(reader: R) -> Result<(bool, bool, BTreeSet<String>)> {
    let mut archive = Archive::new(reader);
    let mut has_native_extensions = false;
    let mut has_embedded_binaries = false;
    let mut languages = BTreeSet::new();

    for entry in archive.entries().context("reading gem data archive")? {
        let mut entry = entry.context("reading file in data archive")?;
        let header = entry.header();
        if !header.entry_type().is_file() {
            continue;
        }
        let path = entry
            .path()
            .context("reading data archive path")?
            .into_owned();
        let path_str = path.to_string_lossy();
        let path_lower = path_str.to_ascii_lowercase();

        if path_lower.starts_with(NATIVE_SOURCE_DIR)
            && let Some(language) = detect_language_from_path(&path_str)
        {
            languages.insert(language.to_string());
        }

        if path_lower.starts_with("ext/") {
            has_native_extensions = true;
        }
        if EMBEDDED_BINARY_DIR_PREFIXES
            .iter()
            .any(|prefix| path_lower.starts_with(prefix))
        {
            has_embedded_binaries = true;
        }

        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            let ext_lower = ext.to_ascii_lowercase();
            if matches!(ext_lower.as_str(), "so" | "dll" | "bundle" | "dylib") {
                has_native_extensions = true;
                has_embedded_binaries = true;
            }
            if matches!(
                ext_lower.as_str(),
                "exe" | "dll" | "so" | "dylib" | "bundle"
            ) {
                has_embedded_binaries = true;
            }
        }

        // `bin/console`, `bin/setup`: scripts without an extension are the
        // norm; only an executable format counts as a shipped binary.
        if !has_embedded_binaries && path_lower.starts_with("bin/") && path.extension().is_none() {
            let mut head = [0u8; 4];
            let read = read_head(&mut entry, &mut head)?;
            has_embedded_binaries = BINARY_MAGIC
                .iter()
                .any(|magic| head[..read].starts_with(magic));
        }
    }

    Ok((has_native_extensions, has_embedded_binaries, languages))
}

/// Up to `head.len()` leading bytes of `reader`; fewer for a shorter file.
fn read_head<R: Read>(reader: &mut R, head: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    while filled < head.len() {
        match reader
            .read(&mut head[filled..])
            .context("reading a bin/ file")?
        {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

pub fn detect_language_from_path(path: &str) -> Option<&'static str> {
    let lower = path.to_ascii_lowercase();

    if lower.ends_with("cargo.toml") || lower.ends_with("build.rs") {
        return Some("Rust");
    }
    if lower.ends_with("extconf.rb") {
        return Some("C");
    }

    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase);

    match ext.as_deref() {
        Some("rs") => Some("Rust"),
        Some("c" | "h") => Some("C"),
        Some("cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx") => Some("C++"),
        Some("go") => Some("Go"),
        Some("java") => Some("Java"),
        Some("swift") => Some("Swift"),
        Some("m") => Some("Objective-C"),
        Some("mm") => Some("Objective-C++"),
        Some("cs") => Some("C#"),
        Some("kt" | "kts") => Some("Kotlin"),
        Some("zig") => Some("Zig"),
        Some("wasm") => Some("WebAssembly"),
        Some("so" | "dll" | "dylib" | "bundle") => Some("Native Binary"),
        _ => None,
    }
}
