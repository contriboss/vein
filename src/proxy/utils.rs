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
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
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

/// Sanitizes a filename by replacing non-alphanumeric characters
pub fn sanitize_filename(input: &str) -> String {
    let sanitized: String = input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "artifact".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // split_name_version_platform tests
    // ============================================================================

    #[test]
    fn parses_simple_gem_name() {
        let parsed = split_name_version_platform("rack-3.0.0").unwrap();
        assert_eq!(parsed.0, "rack");
        assert_eq!(parsed.1, "3.0.0");
        assert!(parsed.2.is_none());
    }

    #[test]
    fn parses_hyphenated_name() {
        let parsed = split_name_version_platform("my-gem-1.0.0").unwrap();
        assert_eq!(parsed.0, "my-gem");
        assert_eq!(parsed.1, "1.0.0");
        assert!(parsed.2.is_none());
    }

    #[test]
    fn parses_platform_suffix() {
        let parsed = split_name_version_platform("nokogiri-1.15.5-x86_64-darwin").unwrap();
        assert_eq!(parsed.0, "nokogiri");
        assert_eq!(parsed.1, "1.15.5");
        assert_eq!(parsed.2.as_deref(), Some("x86_64-darwin"));
    }

    #[test]
    fn parses_prerelease_version() {
        let parsed = split_name_version_platform("rails-7.1.0.rc1").unwrap();
        assert_eq!(parsed.0, "rails");
        assert_eq!(parsed.1, "7.1.0.rc1");
        assert!(parsed.2.is_none());
    }

    #[test]
    fn parses_prerelease_with_platform() {
        let parsed = split_name_version_platform("pg-1.2.3.rc1-x86_64-linux").unwrap();
        assert_eq!(parsed.0, "pg");
        assert_eq!(parsed.1, "1.2.3.rc1");
        assert_eq!(parsed.2.as_deref(), Some("x86_64-linux"));
    }

    #[test]
    fn split_name_version_platform_rejects_no_version() {
        assert!(split_name_version_platform("just-a-name").is_none());
    }

    #[test]
    fn split_name_version_platform_rejects_version_only() {
        assert!(split_name_version_platform("1.0.0").is_none());
    }

    #[test]
    fn split_name_version_platform_handles_single_digit_version() {
        let parsed = split_name_version_platform("gem-0").unwrap();
        assert_eq!(parsed.0, "gem");
        assert_eq!(parsed.1, "0");
        assert!(parsed.2.is_none());
    }

    #[test]
    fn split_name_version_platform_complex_platform() {
        let parsed = split_name_version_platform("nokogiri-1.15.5-x86_64-linux-musl").unwrap();
        assert_eq!(parsed.0, "nokogiri");
        assert_eq!(parsed.1, "1.15.5");
        assert_eq!(parsed.2.as_deref(), Some("x86_64-linux-musl"));
    }

    #[test]
    fn split_name_version_platform_many_hyphens_in_name() {
        let parsed = split_name_version_platform("my-super-long-gem-name-1.2.3").unwrap();
        assert_eq!(parsed.0, "my-super-long-gem-name");
        assert_eq!(parsed.1, "1.2.3");
        assert!(parsed.2.is_none());
    }

    #[test]
    fn split_name_version_platform_beta_version() {
        let parsed = split_name_version_platform("rails-8.0.0.beta1").unwrap();
        assert_eq!(parsed.0, "rails");
        assert_eq!(parsed.1, "8.0.0.beta1");
        assert!(parsed.2.is_none());
    }

    #[test]
    fn split_name_version_platform_java_platform() {
        let parsed = split_name_version_platform("jruby-9.4.0.0-java").unwrap();
        assert_eq!(parsed.0, "jruby");
        assert_eq!(parsed.1, "9.4.0.0");
        assert_eq!(parsed.2.as_deref(), Some("java"));
    }

    #[test]
    fn split_name_version_platform_empty_string() {
        assert!(split_name_version_platform("").is_none());
    }

    #[test]
    fn split_name_version_platform_version_with_letters() {
        let parsed = split_name_version_platform("gem-1.0a").unwrap();
        assert_eq!(parsed.0, "gem");
        assert_eq!(parsed.1, "1.0a");
    }

    #[test]
    fn split_name_version_platform_numeric_name_rejected() {
        assert!(split_name_version_platform("-1.0.0").is_none());
    }
}
