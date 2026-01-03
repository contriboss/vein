use std::{fs::File, io::Read, path::Path};

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use tar::Archive;

use super::symbols::{extract_symbols, RubySymbol};

/// Result of indexing a single Ruby file
#[derive(Debug)]
pub struct FileSymbols {
    pub file_path: String,
    pub symbols: Vec<RubySymbol>,
}

/// Index all Ruby source files in a gem and extract class/module symbols
pub fn index_gem(gem_path: &Path) -> Result<Vec<FileSymbols>> {
    let file =
        File::open(gem_path).with_context(|| format!("opening gem at {}", gem_path.display()))?;
    let mut archive = Archive::new(file);

    let mut results = Vec::new();

    // Find and process data.tar.gz
    for entry in archive.entries().context("reading gem archive entries")? {
        let entry = entry.context("accessing gem archive entry")?;
        let path = entry.path().context("reading entry path")?.into_owned();

        if path.as_os_str().to_string_lossy() == "data.tar.gz" {
            let decoder = GzDecoder::new(entry);
            let symbols = process_data_tar(decoder)?;
            results = symbols;
            break;
        }
    }

    Ok(results)
}

/// Process data.tar.gz and extract symbols from all .rb files
fn process_data_tar<R: Read>(reader: R) -> Result<Vec<FileSymbols>> {
    let mut data_archive = Archive::new(reader);
    let mut results = Vec::new();

    for entry in data_archive
        .entries()
        .context("reading data archive entries")?
    {
        let mut entry = entry.context("accessing data archive entry")?;
        let path = entry.path().context("reading entry path")?.into_owned();
        let path_str = path.to_string_lossy();

        // Only process .rb files
        if !path_str.ends_with(".rb") {
            continue;
        }

        // Read file contents
        let mut contents = String::new();
        if let Err(err) = entry.read_to_string(&mut contents) {
            // Skip files that can't be read as UTF-8
            rama::telemetry::tracing::warn!(
                file = %path_str,
                error = %err,
                "skipping non-UTF-8 Ruby file"
            );
            continue;
        }

        // Extract symbols
        match extract_symbols(&contents) {
            Ok(symbols) => {
                if !symbols.is_empty() {
                    results.push(FileSymbols {
                        file_path: path_str.to_string(),
                        symbols,
                    });
                }
            }
            Err(err) => {
                rama::telemetry::tracing::warn!(
                    file = %path_str,
                    error = %err,
                    "failed to extract symbols"
                );
            }
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn create_test_gem(ruby_files: Vec<(&str, &str)>) -> Result<NamedTempFile> {
        use flate2::{write::GzEncoder, Compression};
        use tar::Builder;

        let temp_gem = NamedTempFile::new()?;

        // Create outer archive
        {
            let mut outer_builder = Builder::new(temp_gem.as_file());

            // Create data.tar.gz in memory
            let data_tar_gz = {
                let mut data_tar = Vec::new();
                let encoder = GzEncoder::new(&mut data_tar, Compression::default());
                let mut data_builder = Builder::new(encoder);

                for (path, content) in ruby_files {
                    let bytes = content.as_bytes();
                    let mut header = tar::Header::new_gnu();
                    header.set_size(bytes.len() as u64);
                    header.set_mode(0o644);
                    header.set_cksum();
                    data_builder.append_data(&mut header, path, bytes)?;
                }

                data_builder.into_inner()?.finish()?;
                data_tar
            };

            // Add data.tar.gz to outer archive
            let mut header = tar::Header::new_gnu();
            header.set_size(data_tar_gz.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            outer_builder.append_data(&mut header, "data.tar.gz", &data_tar_gz[..])?;

            outer_builder.finish()?;
        } // outer_builder dropped here

        Ok(temp_gem)
    }

    #[test]
    fn test_index_gem_simple() -> Result<()> {
        let gem = create_test_gem(vec![
            ("lib/foo.rb", "class Foo\nend\n"),
            ("lib/bar.rb", "module Bar\nend\n"),
        ])?;

        let results = index_gem(gem.path())?;

        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|f| f.file_path.contains("foo.rb")));
        assert!(results.iter().any(|f| f.file_path.contains("bar.rb")));

        Ok(())
    }

    #[test]
    fn test_index_gem_skips_non_rb_files() -> Result<()> {
        let gem = create_test_gem(vec![
            ("lib/foo.rb", "class Foo\nend\n"),
            ("README.md", "# Readme\n"),
            ("lib/foo.c", "int main() {}\n"),
        ])?;

        let results = index_gem(gem.path())?;

        assert_eq!(results.len(), 1);
        assert!(results[0].file_path.contains("foo.rb"));

        Ok(())
    }
}
