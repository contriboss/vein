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
    assert_eq!(req.storage_path(), "npm_index/lodash/versions/4.17.21.json");
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
