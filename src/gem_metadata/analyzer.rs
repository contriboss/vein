use std::{collections::BTreeSet, fs::File, io::Read, path::Path};

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use tar::Archive;

use super::binary_arch::{detect_binary_arch, matches_platform, BinaryInfo};

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

#[derive(Debug, Clone)]
pub struct ArchValidation {
    pub claimed_platform: Option<String>,
    pub detected_binaries: Vec<(String, BinaryInfo)>,
    pub is_valid: bool,
    pub mismatches: Vec<String>,
}

/// Validate that binary architectures in a gem match its claimed platform
pub fn validate_binary_architectures(
    gem_path: &Path,
    claimed_platform: Option<&str>,
) -> Result<ArchValidation> {
    let file = File::open(gem_path)
        .with_context(|| format!("opening gem file: {}", gem_path.display()))?;
    let mut archive = Archive::new(file);

    let mut detected_binaries = Vec::new();
    let mut mismatches = Vec::new();

    // Find data.tar.gz in the outer archive
    for entry in archive.entries()? {
        let entry = entry?;
        let path = entry.path()?.into_owned();

        if path.as_os_str().to_string_lossy() == "data.tar.gz" {
            let decoder = GzDecoder::new(entry);
            let mut data_archive = Archive::new(decoder);

            // Scan all files in data.tar.gz for binaries
            for data_entry in data_archive.entries()? {
                let mut data_entry = data_entry?;
                let file_path = data_entry.path()?.into_owned();
                let path_str = file_path.to_string_lossy();

                // Check if this is a binary file by extension or location
                let path_str_lower = path_str.to_ascii_lowercase();
                let is_binary = file_path
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|ext| matches!(ext, "so" | "dll" | "dylib" | "bundle" | "exe"))
                    .unwrap_or(false)
                    || path_str_lower.starts_with("exe/")
                    || path_str_lower.starts_with("bin/");

                if is_binary {
                    // Read the file contents
                    let mut contents = Vec::new();
                    if let Err(_) = data_entry.read_to_end(&mut contents) {
                        continue; // Skip files we can't read
                    }

                    // Detect architecture
                    if let Ok(binary_info) = detect_binary_arch(&contents) {
                        let is_match = matches_platform(claimed_platform, &binary_info);

                        if !is_match {
                            mismatches.push(path_str.to_string());
                        }

                        detected_binaries.push((path_str.to_string(), binary_info));
                    }
                }
            }

            break;
        }
    }

    let is_valid = mismatches.is_empty();

    Ok(ArchValidation {
        claimed_platform: claimed_platform.map(|s| s.to_string()),
        detected_binaries,
        is_valid,
        mismatches,
    })
}
