use super::*;

#[test]
fn test_index_path() {
    assert_eq!(index_path("a"), Some("1/a".to_string()));
    assert_eq!(index_path("ab"), Some("2/ab".to_string()));
    assert_eq!(index_path("abc"), Some("3/a/abc".to_string()));
    assert_eq!(index_path("abcd"), Some("ab/cd/abcd".to_string()));
    assert_eq!(index_path("serde"), Some("se/rd/serde".to_string()));
    assert_eq!(index_path("Tokio"), Some("to/ki/tokio".to_string()));
    assert_eq!(index_path(""), None);
}

#[test]
fn test_index_path_rejects_non_crate_names() {
    for name in ["....", "../x", "a.b", "a/b", "ünï", &"a".repeat(65)] {
        assert_eq!(index_path(name), None, "{name}");
    }
}
