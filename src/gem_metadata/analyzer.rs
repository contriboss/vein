use std::{collections::BTreeSet, io::Read};

use anyhow::{Context, Result};
use tar::Archive;

const EMBEDDED_BINARY_DIR_PREFIXES: &[&str] = &["vendor/", "libexec/", "resources/"];

pub fn analyze_data_tar<R: Read>(reader: R) -> Result<(bool, bool, BTreeSet<String>)> {
    let mut archive = Archive::new(reader);
    let mut has_native_extensions = false;
    let mut has_embedded_binaries = false;
    let mut languages = BTreeSet::new();

    for entry in archive.entries().context("reading gem data archive")? {
        let entry = entry.context("reading file in data archive")?;
        let header = entry.header();
        if !header.entry_type().is_file() {
            continue;
        }
        let path = entry.path().context("reading data archive path")?;
        let path_str = path.to_string_lossy();
        let path_lower = path_str.to_ascii_lowercase();

        if let Some(language) = detect_language_from_path(&path_str) {
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

        if !has_embedded_binaries && path_lower.starts_with("bin/") {
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                let ext_lower = ext.to_ascii_lowercase();
                if !matches!(
                    ext_lower.as_str(),
                    "rb" | "erb" | "rake" | "sh" | "bat" | "ps1"
                ) {
                    has_embedded_binaries = true;
                }
            } else {
                has_embedded_binaries = true;
            }
        }

        if has_native_extensions && has_embedded_binaries {
            break;
        }
    }

    Ok((has_native_extensions, has_embedded_binaries, languages))
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
        .map(|s| s.to_ascii_lowercase());

    match ext.as_deref() {
        Some("rs") => Some("Rust"),
        Some("c") | Some("h") => Some("C"),
        Some("cc") | Some("cpp") | Some("cxx") | Some("hpp") | Some("hh") | Some("hxx") => {
            Some("C++")
        }
        Some("go") => Some("Go"),
        Some("java") => Some("Java"),
        Some("swift") => Some("Swift"),
        Some("m") => Some("Objective-C"),
        Some("mm") => Some("Objective-C++"),
        Some("cs") => Some("C#"),
        Some("kt") | Some("kts") => Some("Kotlin"),
        Some("zig") => Some("Zig"),
        Some("wasm") => Some("WebAssembly"),
        Some("so") | Some("dll") | Some("dylib") | Some("bundle") => Some("Native Binary"),
        _ => None,
    }
}
