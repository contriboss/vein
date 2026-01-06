//! Ruby-style version requirement parsing and matching.
//!
//! Supports:
//! - `>= 1.0` - greater than or equal
//! - `> 1.0` - greater than
//! - `< 2.0` - less than
//! - `<= 2.0` - less than or equal
//! - `= 1.0.0` - exact match
//! - `!= 1.0.0` - not equal
//! - `~> 1.5` - pessimistic (>= 1.5.0, < 2.0.0)
//! - `~> 1.5.3` - pessimistic (>= 1.5.3, < 1.6.0)
//! - Compound: `>= 1.0, < 2.0`

use semver::Version;

/// Parse a version string into a semver Version, handling Ruby's flexible format.
/// Ruby allows versions like "1.0" which semver requires as "1.0.0".
fn parse_version(version: &str) -> Option<Version> {
    let trimmed = version.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Try parsing as-is first
    if let Ok(v) = Version::parse(trimmed) {
        return Some(v);
    }

    // Ruby versions can be just "1" or "1.0", normalize to "x.y.z"
    let parts: Vec<&str> = trimmed.split('.').collect();
    let normalized = match parts.len() {
        1 => format!("{}.0.0", parts[0]),
        2 => format!("{}.{}.0", parts[0], parts[1]),
        _ => {
            // Take first 3 parts, ignore pre-release for now
            let base = parts.iter().take(3).cloned().collect::<Vec<_>>().join(".");
            base
        }
    };

    Version::parse(&normalized).ok()
}

/// A single version constraint (operator + version).
#[derive(Debug, Clone)]
struct Constraint {
    op: Op,
    version: Version,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Eq,    // =
    Ne,    // !=
    Gt,    // >
    Lt,    // <
    Ge,    // >=
    Le,    // <=
    Tilde, // ~> (pessimistic)
}

impl Constraint {
    fn parse(s: &str) -> Option<Self> {
        let s = s.trim();

        // Parse operator
        let (op, rest) = if s.starts_with(">=") {
            (Op::Ge, s[2..].trim())
        } else if s.starts_with("<=") {
            (Op::Le, s[2..].trim())
        } else if s.starts_with("~>") {
            (Op::Tilde, s[2..].trim())
        } else if s.starts_with("!=") {
            (Op::Ne, s[2..].trim())
        } else if s.starts_with('>') {
            (Op::Gt, s[1..].trim())
        } else if s.starts_with('<') {
            (Op::Lt, s[1..].trim())
        } else if s.starts_with('=') {
            (Op::Eq, s[1..].trim())
        } else {
            // No operator means exact match
            (Op::Eq, s)
        };

        let version = parse_version(rest)?;
        Some(Self { op, version })
    }

    fn matches(&self, candidate: &Version) -> bool {
        match self.op {
            Op::Eq => candidate == &self.version,
            Op::Ne => candidate != &self.version,
            Op::Gt => candidate > &self.version,
            Op::Lt => candidate < &self.version,
            Op::Ge => candidate >= &self.version,
            Op::Le => candidate <= &self.version,
            Op::Tilde => {
                // Pessimistic version constraint
                // ~> 1.5 means >= 1.5.0 and < 2.0.0
                // ~> 1.5.3 means >= 1.5.3 and < 1.6.0
                if candidate < &self.version {
                    return false;
                }

                // Determine upper bound based on version specificity
                // If version has non-zero patch, bump minor
                // Otherwise bump major
                let upper = if self.version.patch > 0 {
                    Version::new(self.version.major, self.version.minor + 1, 0)
                } else if self.version.minor > 0 {
                    Version::new(self.version.major + 1, 0, 0)
                } else {
                    Version::new(self.version.major + 1, 0, 0)
                };

                candidate < &upper
            }
        }
    }
}

/// Check if a version matches a Ruby-style requirement string.
///
/// # Examples
/// ```ignore
/// assert!(matches_requirement("1.5.0", ">= 1.0"));
/// assert!(matches_requirement("1.5.0", "~> 1.4"));
/// assert!(!matches_requirement("2.0.0", "~> 1.4"));
/// assert!(matches_requirement("1.5.0", ">= 1.0, < 2.0"));
/// ```
pub fn matches_requirement(version: &str, requirement: &str) -> bool {
    let candidate = match parse_version(version) {
        Some(v) => v,
        None => return false,
    };

    // Handle ">= 0" or empty requirement as "any version"
    let req = requirement.trim();
    if req.is_empty() || req == ">= 0" {
        return true;
    }

    // Split on comma for compound requirements
    let constraints: Vec<&str> = req.split(',').map(|s| s.trim()).collect();

    // All constraints must match
    for constraint_str in constraints {
        if constraint_str.is_empty() {
            continue;
        }
        let constraint = match Constraint::parse(constraint_str) {
            Some(c) => c,
            None => return false, // Invalid constraint
        };
        if !constraint.matches(&candidate) {
            return false;
        }
    }

    true
}

/// Find the latest version from a list that matches a requirement.
///
/// Versions are sorted in descending order (newest first) and the first match is returned.
pub fn find_latest_matching(versions: &[String], requirement: &str) -> Option<String> {
    // Parse and sort versions in descending order
    let mut parsed: Vec<(Version, &String)> = versions
        .iter()
        .filter_map(|v| parse_version(v).map(|parsed| (parsed, v)))
        .collect();

    parsed.sort_by(|a, b| b.0.cmp(&a.0)); // Descending order

    // Return first matching version
    for (parsed_ver, original) in parsed {
        if matches_requirement(&parsed_ver.to_string(), requirement) {
            return Some((*original).clone());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version() {
        assert_eq!(parse_version("1.0.0"), Some(Version::new(1, 0, 0)));
        assert_eq!(parse_version("1.0"), Some(Version::new(1, 0, 0)));
        assert_eq!(parse_version("1"), Some(Version::new(1, 0, 0)));
        assert_eq!(parse_version("2.5.3"), Some(Version::new(2, 5, 3)));
    }

    #[test]
    fn test_matches_ge() {
        assert!(matches_requirement("1.5.0", ">= 1.0"));
        assert!(matches_requirement("1.0.0", ">= 1.0"));
        assert!(!matches_requirement("0.9.0", ">= 1.0"));
    }

    #[test]
    fn test_matches_lt() {
        assert!(matches_requirement("1.5.0", "< 2.0"));
        assert!(!matches_requirement("2.0.0", "< 2.0"));
        assert!(!matches_requirement("2.1.0", "< 2.0"));
    }

    #[test]
    fn test_matches_tilde_minor() {
        // ~> 1.5 means >= 1.5.0 and < 2.0.0
        assert!(matches_requirement("1.5.0", "~> 1.5"));
        assert!(matches_requirement("1.9.9", "~> 1.5"));
        assert!(!matches_requirement("1.4.9", "~> 1.5"));
        assert!(!matches_requirement("2.0.0", "~> 1.5"));
    }

    #[test]
    fn test_matches_tilde_patch() {
        // ~> 1.5.3 means >= 1.5.3 and < 1.6.0
        assert!(matches_requirement("1.5.3", "~> 1.5.3"));
        assert!(matches_requirement("1.5.9", "~> 1.5.3"));
        assert!(!matches_requirement("1.5.2", "~> 1.5.3"));
        assert!(!matches_requirement("1.6.0", "~> 1.5.3"));
    }

    #[test]
    fn test_matches_compound() {
        assert!(matches_requirement("1.5.0", ">= 1.0, < 2.0"));
        assert!(!matches_requirement("0.9.0", ">= 1.0, < 2.0"));
        assert!(!matches_requirement("2.0.0", ">= 1.0, < 2.0"));
    }

    #[test]
    fn test_matches_any() {
        assert!(matches_requirement("1.0.0", ">= 0"));
        assert!(matches_requirement("999.0.0", ">= 0"));
        assert!(matches_requirement("0.0.1", ""));
    }

    #[test]
    fn test_find_latest_matching() {
        let versions = vec![
            "1.0.0".to_string(),
            "1.5.0".to_string(),
            "2.0.0".to_string(),
            "1.8.0".to_string(),
        ];

        assert_eq!(
            find_latest_matching(&versions, "~> 1.5"),
            Some("1.8.0".to_string())
        );
        assert_eq!(
            find_latest_matching(&versions, ">= 2.0"),
            Some("2.0.0".to_string())
        );
        assert_eq!(
            find_latest_matching(&versions, "< 1.5"),
            Some("1.0.0".to_string())
        );
        assert_eq!(find_latest_matching(&versions, ">= 3.0"), None);
    }
}
