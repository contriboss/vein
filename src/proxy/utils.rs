/// Splits a gem filename stem into (name, version, platform)
///
/// Examples:
/// - "rack-3.0.0" -> ("rack", "3.0.0", None)
/// - "nokogiri-1.15.5-x86_64-darwin" -> ("nokogiri", "1.15.5", Some("x86_64-darwin"))
pub fn split_name_version_platform(stem: &str) -> Option<(String, String, Option<String>)> {
    let parts: Vec<&str> = stem.split('-').collect();
    for idx in 1..parts.len() {
        if parts[idx]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
        {
            let name = parts[..idx].join("-");
            let version = parts[idx].to_string();
            let platform = if idx + 1 < parts.len() {
                Some(parts[idx + 1..].join("-"))
            } else {
                None
            };
            if name.is_empty() || version.is_empty() {
                continue;
            }
            return Some((name, version, platform));
        }
    }
    None
}

pub use crate::util::sanitize_filename;

#[cfg(test)]
mod tests;
