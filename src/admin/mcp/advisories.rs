//! Advisories next to the packages and versions an agent reads: only those
//! affecting what the answer shows, so a clean package adds nothing.

use rama::telemetry::tracing::warn;
use serde::Serialize;
use vein_adapter::{Advisory, AssetKind};

use super::VeinMcp;
use crate::advisories::{affects, requirements};
use crate::untrusted::{SUMMARY_LIMIT, clean};

/// One advisory, as `/mcp` shows it; the full text is at
/// `https://github.com/advisories/{id}`.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Note {
    pub id: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cve: Vec<String>,
    /// `low`, `moderate`, `high` or `critical`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    /// As its reporter wrote it, cleaned: data, never instructions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// The versions it affects, as requirements (`>= 1.2, < 1.4.1`).
    pub vulnerable: Vec<String>,
    /// Which of the versions this answer lists it affects.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub affects: Vec<String>,
    /// For one version: the release that fixes it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixed_in: Option<String>,
}

impl VeinMcp {
    /// Every advisory about package `name`; none when they cannot be read
    /// (logged): an advisory lookup never fails the read it annotates.
    pub(super) async fn advisories(&self, kind: AssetKind, name: &str) -> Vec<Advisory> {
        self.resources
            .advisories(kind, &[name.to_owned()])
            .await
            .unwrap_or_else(|err| {
                warn!(name, "advisories: lookup failed: {err:#}");
                Vec::new()
            })
    }
}

/// The advisories affecting any of `versions`, each naming which.
pub fn for_versions(kind: AssetKind, advisories: &[Advisory], versions: &[&str]) -> Vec<Note> {
    advisories
        .iter()
        .filter_map(|advisory| {
            let affected = &advisory.packages.first()?.affected;
            let mut hit: Vec<String> = Vec::new();
            for &version in versions {
                if affects(kind, affected, version).is_some() && !hit.iter().any(|v| v == version) {
                    hit.push(version.to_owned());
                }
            }
            (!hit.is_empty()).then(|| Note {
                affects: hit,
                ..note(advisory)
            })
        })
        .collect()
}

/// The advisories affecting `version`, each with its fix.
pub fn for_version(kind: AssetKind, advisories: &[Advisory], version: &str) -> Vec<Note> {
    advisories
        .iter()
        .filter_map(|advisory| {
            let hit = affects(kind, &advisory.packages.first()?.affected, version)?;
            Some(Note {
                fixed_in: hit.fixed_in,
                ..note(advisory)
            })
        })
        .collect()
}

/// Every advisory, for a package's full version list: without summaries,
/// which an old, popular package would have by the dozen.
pub fn every(advisories: &[Advisory]) -> Vec<Note> {
    advisories
        .iter()
        .map(|advisory| Note {
            summary: None,
            ..note(advisory)
        })
        .collect()
}

fn note(advisory: &Advisory) -> Note {
    Note {
        id: advisory.id.clone(),
        cve: advisory
            .aliases
            .iter()
            .filter(|alias| alias.starts_with("CVE-"))
            .cloned()
            .collect(),
        severity: advisory.severity.as_deref().map(str::to_ascii_lowercase),
        summary: advisory
            .summary
            .as_deref()
            .and_then(|summary| clean(summary, SUMMARY_LIMIT)),
        vulnerable: advisory
            .packages
            .first()
            .map(|package| requirements(&package.affected))
            .unwrap_or_default(),
        affects: Vec::new(),
        fixed_in: None,
    }
}

#[cfg(test)]
mod tests;
