use super::*;

#[test]
fn build_conditional_headers_sets_etag_and_last_modified() {
    let meta = CacheEntryMeta {
        etag: Some("\"abc\"".to_string()),
        last_modified: Some("Wed, 01 Jan 2025 00:00:00 GMT".to_string()),
    };
    let headers = build_conditional_headers(Some(&meta));
    assert_eq!(headers.get(header::IF_NONE_MATCH).unwrap(), "\"abc\"");
    assert_eq!(
        headers.get(header::IF_MODIFIED_SINCE).unwrap(),
        "Wed, 01 Jan 2025 00:00:00 GMT"
    );
}

#[test]
fn build_conditional_headers_empty_without_meta() {
    let headers = build_conditional_headers(None);
    assert!(headers.is_empty());
}
