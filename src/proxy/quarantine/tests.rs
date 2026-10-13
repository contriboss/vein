use super::*;

#[test]
fn test_parse_compact_line_simple() {
    let (key, rest) = parse_compact_line("1.0.0 |abc123|dep1").unwrap();
    assert_eq!(key, "1.0.0");
    assert_eq!(rest, "|abc123|dep1");
}

#[test]
fn test_parse_compact_line_with_platform() {
    let (key, rest) = parse_compact_line("1.0.0 x86_64-linux|abc123|dep1").unwrap();
    assert_eq!(key, "1.0.0:x86_64-linux");
    assert_eq!(rest, "|abc123|dep1");
}

#[test]
fn test_parse_compact_line_ruby_platform() {
    let (key, _) = parse_compact_line("1.0.0 ruby|abc123|").unwrap();
    assert_eq!(key, "1.0.0"); // ruby platform treated as no platform
}

#[test]
fn test_format_version_key() {
    assert_eq!(format_version_key("1.0.0", None), "1.0.0");
    assert_eq!(format_version_key("1.0.0", Some("ruby")), "1.0.0");
    assert_eq!(
        format_version_key("1.0.0", Some("x86_64-linux")),
        "1.0.0:x86_64-linux"
    );
}
