//! NPM package types and path parsing
//!
//! Handles both scoped (@org/package) and unscoped packages.

use percent_encoding::percent_decode_str;

/// Represents a parsed npm package request
#[derive(Debug, Clone)]
pub struct NpmPackageRequest {
    /// Package name (e.g., "lodash" or "@types/node")
    pub name: String,
    /// Specific version if requested
    pub version: Option<String>,
    /// Whether this is a tarball download request
    pub is_tarball: bool,
    /// Tarball filename if applicable
    pub tarball_name: Option<String>,
}

impl NpmPackageRequest {
    /// Parse a package path from URL
    ///
    /// Handles:
    /// - `/lodash` - unscoped package metadata
    /// - `/@scope/package` - scoped package metadata
    /// - `/lodash/-/lodash-1.0.0.tgz` - unscoped tarball
    /// - `/@scope/package/-/package-1.0.0.tgz` - scoped tarball
    pub fn from_path(path: &str) -> Option<Self> {
        let trimmed = path.trim_start_matches('/');
        if trimmed.is_empty() {
            return None;
        }

        // Decode URL encoding (e.g., %40 -> @, %2f -> /)
        let decoded = percent_decode_str(trimmed).decode_utf8().ok()?;
        let decoded = decoded.trim_start_matches('/');

        if decoded.is_empty() {
            return None;
        }

        // Check if this is a tarball request (contains /-/)
        if decoded.contains("/-/") {
            return Self::parse_tarball_path(decoded);
        }

        // Reject path traversal and invalid separators after decoding for non-tarball paths
        if is_invalid_path(decoded) {
            return None;
        }

        // Parse as metadata request
        Self::parse_metadata_path(decoded)
    }

    fn parse_metadata_path(decoded: &str) -> Option<Self> {
        // Check for scoped package (@scope/name)
        let (name, version) = if decoded.starts_with('@') {
            // Scoped: @scope/name or @scope/name/version
            let parts: Vec<&str> = decoded.splitn(3, '/').collect();
            if parts.len() < 2 {
                return None;
            }
            let scope = parts[0];
            let pkg_name = parts[1];
            let version = parts
                .get(2)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());

            // Validate scope and name
            if !is_valid_package_name(scope) || !is_valid_package_name(pkg_name) {
                return None;
            }

            (format!("{}/{}", scope, pkg_name), version)
        } else {
            // Unscoped: name or name/version
            let parts: Vec<&str> = decoded.splitn(2, '/').collect();
            let pkg_name = parts[0];
            let version = parts
                .get(1)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());

            if !is_valid_package_name(pkg_name) {
                return None;
            }

            (pkg_name.to_string(), version)
        };

        if let Some(version) = version.as_deref()
            && !is_valid_version_segment(version)
        {
            return None;
        }

        Some(Self {
            name,
            version,
            is_tarball: false,
            tarball_name: None,
        })
    }

    fn parse_tarball_path(decoded: &str) -> Option<Self> {
        // Format: {name}/-/{filename}.tgz or @{scope}/{name}/-/{filename}.tgz
        let parts: Vec<&str> = decoded.split("/-/").collect();
        if parts.len() != 2 {
            return None;
        }

        let name_part = parts[0];
        let tarball = parts[1];

        // Validate tarball filename
        if !is_valid_tarball_name(tarball) {
            return None;
        }

        // Parse package name
        let name = if name_part.starts_with('@') {
            // Scoped package
            let scope_parts: Vec<&str> = name_part.splitn(2, '/').collect();
            if scope_parts.len() != 2 {
                return None;
            }
            if !is_valid_package_name(scope_parts[0]) || !is_valid_package_name(scope_parts[1]) {
                return None;
            }
            format!("{}/{}", scope_parts[0], scope_parts[1])
        } else {
            if !is_valid_package_name(name_part) {
                return None;
            }
            name_part.to_string()
        };

        // Extract version from tarball name
        // Format: {short_name}-{version}.tgz
        let short_name = name.rsplit('/').next().unwrap_or(&name);
        let tarball_stem = tarball.strip_suffix(".tgz")?;
        let version = tarball_stem.strip_prefix(&format!("{}-", short_name))?;

        if !is_valid_version_segment(version) {
            return None;
        }

        Some(Self {
            name,
            version: Some(version.to_string()),
            is_tarball: true,
            tarball_name: Some(tarball.to_string()),
        })
    }

    /// Get storage path for caching
    pub fn storage_path(&self) -> String {
        let safe_name = sanitize_segment(&self.name);
        if self.is_tarball {
            // npm/{name}/{tarball}
            let safe_tarball = sanitize_segment(
                self.tarball_name.as_deref().unwrap_or("unknown.tgz"),
            );
            format!(
                "npm/{}/{}",
                safe_name,
                safe_tarball
            )
        } else {
            // npm_index/{name}/metadata.json or npm_index/{name}/versions/{version}.json
            if let Some(version) = self.version.as_deref() {
                let safe_version = sanitize_segment(version);
                format!("npm_index/{}/versions/{}.json", safe_name, safe_version)
            } else {
                format!("npm_index/{}/metadata.json", safe_name)
            }
        }
    }

    /// Get meta key for cache backend
    pub fn meta_key(&self) -> String {
        if self.is_tarball {
            format!(
                "npm:tarball:{}:{}",
                self.name,
                self.version.as_deref().unwrap_or("unknown")
            )
        } else if let Some(version) = self.version.as_deref() {
            format!("npm:metadata:{}:{}", self.name, version)
        } else {
            format!("npm:metadata:{}", self.name)
        }
    }
}

fn is_invalid_path(decoded: &str) -> bool {
    decoded.contains('\\') || decoded.split('/').any(|segment| {
        segment.is_empty() || segment == "." || segment == ".."
    })
}

fn is_valid_version_segment(segment: &str) -> bool {
    if !is_safe_segment(segment) {
        return false;
    }
    segment
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
}

fn is_valid_tarball_name(name: &str) -> bool {
    if !is_safe_segment(name) || !name.ends_with(".tgz") {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
}

fn is_safe_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && !segment.contains('/')
        && !segment.contains('\\')
        && segment.chars().all(|c| c.is_ascii_graphic())
}

fn sanitize_segment(input: &str) -> String {
    let sanitized: String = input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@' | '+') {
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

/// Validate npm package name component
fn is_valid_package_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 214 {
        return false;
    }

    // Can start with @ for scopes
    let check_name = name.strip_prefix('@').unwrap_or(name);

    // Must not start with . or _
    if check_name.starts_with('.') || check_name.starts_with('_') {
        return false;
    }

    // Only allowed characters
    check_name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_unscoped_metadata() {
        let req = NpmPackageRequest::from_path("/lodash").unwrap();
        assert_eq!(req.name, "lodash");
        assert!(req.version.is_none());
        assert!(!req.is_tarball);
    }

    #[test]
    fn test_parse_unscoped_with_version() {
        let req = NpmPackageRequest::from_path("/lodash/4.17.21").unwrap();
        assert_eq!(req.name, "lodash");
        assert_eq!(req.version, Some("4.17.21".to_string()));
        assert!(!req.is_tarball);
    }

    #[test]
    fn test_parse_scoped_metadata() {
        let req = NpmPackageRequest::from_path("/@types/node").unwrap();
        assert_eq!(req.name, "@types/node");
        assert!(req.version.is_none());
        assert!(!req.is_tarball);
    }

    #[test]
    fn test_parse_scoped_encoded() {
        let req = NpmPackageRequest::from_path("/@types%2fnode").unwrap();
        assert_eq!(req.name, "@types/node");
        assert!(!req.is_tarball);
    }

    #[test]
    fn test_parse_unscoped_tarball() {
        let req = NpmPackageRequest::from_path("/lodash/-/lodash-4.17.21.tgz").unwrap();
        assert_eq!(req.name, "lodash");
        assert_eq!(req.version, Some("4.17.21".to_string()));
        assert!(req.is_tarball);
        assert_eq!(req.tarball_name, Some("lodash-4.17.21.tgz".to_string()));
    }

    #[test]
    fn test_parse_scoped_tarball() {
        let req = NpmPackageRequest::from_path("/@types/node/-/node-18.0.0.tgz").unwrap();
        assert_eq!(req.name, "@types/node");
        assert_eq!(req.version, Some("18.0.0".to_string()));
        assert!(req.is_tarball);
    }

    #[test]
    fn test_storage_path_metadata() {
        let req = NpmPackageRequest::from_path("/@types/node").unwrap();
        assert_eq!(req.storage_path(), "npm_index/@types_node/metadata.json");
    }

    #[test]
    fn test_storage_path_metadata_version() {
        let req = NpmPackageRequest::from_path("/lodash/4.17.21").unwrap();
        assert_eq!(
            req.storage_path(),
            "npm_index/lodash/versions/4.17.21.json"
        );
    }

    #[test]
    fn test_storage_path_tarball() {
        let req = NpmPackageRequest::from_path("/lodash/-/lodash-4.17.21.tgz").unwrap();
        assert_eq!(req.storage_path(), "npm/lodash/lodash-4.17.21.tgz");
    }

    #[test]
    fn test_reject_path_traversal() {
        assert!(NpmPackageRequest::from_path("/../etc/passwd").is_none());
        assert!(NpmPackageRequest::from_path("/foo/../bar").is_none());
        assert!(NpmPackageRequest::from_path("/..%2fetc/passwd").is_none());
        assert!(NpmPackageRequest::from_path("/%2e%2e/secret").is_none());
    }

    #[test]
    fn test_reject_empty() {
        assert!(NpmPackageRequest::from_path("/").is_none());
        assert!(NpmPackageRequest::from_path("").is_none());
    }
}
