use std::cmp::Ordering;

use super::{
    GemVersion, QuarantineStats,
    types::{IndexStats, SbomCoverage},
};

pub fn build_index_stats(
    total_assets: i64,
    rubygems_assets: i64,
    crate_assets: i64,
    npm_assets: i64,
    unique_packages: i64,
    total_size_bytes: i64,
    last_accessed: Option<String>,
) -> IndexStats {
    IndexStats {
        total_assets: clamp_count(total_assets),
        rubygems_assets: clamp_count(rubygems_assets),
        crate_assets: clamp_count(crate_assets),
        npm_assets: clamp_count(npm_assets),
        unique_packages: clamp_count(unique_packages),
        total_size_bytes: clamp_count(total_size_bytes),
        last_accessed,
    }
}

pub fn build_sbom_coverage(total: i64, with_sbom: i64) -> SbomCoverage {
    SbomCoverage {
        metadata_rows: clamp_count(total),
        with_sbom: clamp_count(with_sbom),
    }
}

pub fn build_quarantine_stats(
    quarantined: i64,
    available: i64,
    yanked: i64,
    pinned: i64,
    releasing_today: i64,
    releasing_week: i64,
) -> QuarantineStats {
    QuarantineStats {
        total_quarantined: clamp_count(quarantined),
        total_available: clamp_count(available),
        total_yanked: clamp_count(yanked),
        total_pinned: clamp_count(pinned),
        versions_releasing_today: clamp_count(releasing_today),
        versions_releasing_this_week: clamp_count(releasing_week),
    }
}

pub fn into_gem_versions<T>(rows: Vec<T>) -> Vec<GemVersion>
where
    T: Into<GemVersion>,
{
    rows.into_iter().map(Into::into).collect()
}

pub fn latest_gem_version<T>(rows: Vec<T>) -> Option<GemVersion>
where
    T: Into<GemVersion>,
{
    let mut versions = into_gem_versions(rows);
    versions.sort_by(|a, b| compare_versions(&b.version, &a.version));
    versions.into_iter().next()
}

pub fn search_like_pattern(query: &str) -> String {
    format!("%{}%", query)
}

pub fn json_array_like_pattern(value: &str) -> String {
    format!("%\"{}\"%", value)
}

fn clamp_count(value: i64) -> u64 {
    value.max(0) as u64
}

fn compare_versions(a: &str, b: &str) -> Ordering {
    match (semver::Version::parse(a), semver::Version::parse(b)) {
        (Ok(va), Ok(vb)) => va.cmp(&vb),
        _ => a.cmp(b),
    }
}
