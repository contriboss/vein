use super::*;

fn sorted(mut versions: Vec<&str>) -> Vec<&str> {
    versions.sort_by(|a, b| compare_gem_versions(a, b));
    versions
}

#[test]
fn numeric_segments_compare_as_numbers() {
    assert_eq!(
        sorted(vec!["1.10.0", "1.9.0", "1.2.0", "10.0"]),
        ["1.2.0", "1.9.0", "1.10.0", "10.0"]
    );
}

#[test]
fn four_segment_versions_order_after_their_base() {
    assert_eq!(
        sorted(vec!["7.1.3.4", "7.1.3", "7.1.10"]),
        ["7.1.3", "7.1.3.4", "7.1.10"]
    );
}

#[test]
fn prereleases_sort_before_their_release() {
    assert_eq!(
        sorted(vec!["1.0.0", "1.0.0.rc2", "1.0.0.beta", "1.0.0.rc1", "0.9"]),
        ["0.9", "1.0.0.beta", "1.0.0.rc1", "1.0.0.rc2", "1.0.0"]
    );
}

#[test]
fn trailing_zeros_are_equal() {
    assert_eq!(compare_gem_versions("1.0", "1.0.0"), Ordering::Equal);
    assert_eq!(compare_gem_versions("2", "2.0.0.0"), Ordering::Equal);
}
