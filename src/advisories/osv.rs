//! Reading one advisory in OSV format, as GitHub's database writes them.

use anyhow::{Context, Result};
use jiff::Timestamp;
use serde::Deserialize;
use vein_adapter::{Advisory, Affected, AffectedPackage, AssetKind, Interval};

#[derive(Deserialize)]
struct Osv {
    id: String,
    modified: Timestamp,
    published: Option<Timestamp>,
    withdrawn: Option<Timestamp>,
    #[serde(default)]
    aliases: Vec<String>,
    summary: Option<String>,
    #[serde(default)]
    affected: Vec<OsvAffected>,
    database_specific: Option<DatabaseSpecific>,
}

#[derive(Deserialize)]
struct OsvAffected {
    package: Option<OsvPackage>,
    #[serde(default)]
    ranges: Vec<OsvRange>,
    #[serde(default)]
    versions: Vec<String>,
}

#[derive(Deserialize)]
struct OsvPackage {
    ecosystem: String,
    name: String,
}

#[derive(Deserialize)]
struct OsvRange {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    events: Vec<Event>,
}

#[derive(Deserialize)]
struct Event {
    introduced: Option<String>,
    fixed: Option<String>,
    last_affected: Option<String>,
}

#[derive(Deserialize)]
struct DatabaseSpecific {
    severity: Option<String>,
}

/// The advisory in `json`, keeping only the packages of registries vein
/// serves; `None` when it affects none of them or was withdrawn.
pub fn parse(json: &str) -> Result<Option<Advisory>> {
    let osv: Osv = serde_json::from_str(json).context("reading an OSV advisory")?;
    if osv.withdrawn.is_some() {
        return Ok(None);
    }
    let mut packages: Vec<AffectedPackage> = Vec::new();
    for entry in osv.affected {
        let Some(package) = entry.package else {
            continue;
        };
        let Some(kind) = registry(&package.ecosystem) else {
            continue;
        };
        let affected = Affected {
            intervals: entry
                .ranges
                .iter()
                .filter(|range| range.kind != "GIT")
                .flat_map(intervals)
                .collect(),
            versions: entry.versions,
        };
        // An advisory lists a package once per range it was reported for.
        match packages
            .iter_mut()
            .find(|known| known.kind == kind && known.name == package.name)
        {
            Some(known) => {
                known.affected.intervals.extend(affected.intervals);
                known.affected.versions.extend(affected.versions);
            }
            None => packages.push(AffectedPackage {
                kind,
                name: package.name,
                affected,
            }),
        }
    }
    if packages.is_empty() {
        return Ok(None);
    }
    Ok(Some(Advisory {
        id: osv.id,
        aliases: osv.aliases,
        severity: osv
            .database_specific
            .and_then(|specific| specific.severity)
            .map(|severity| severity.to_ascii_uppercase()),
        summary: osv.summary,
        published: osv.published,
        modified: osv.modified,
        packages,
    }))
}

/// The registry of an OSV ecosystem, if vein serves it.
fn registry(ecosystem: &str) -> Option<AssetKind> {
    match ecosystem {
        "RubyGems" => Some(AssetKind::Gem),
        "crates.io" => Some(AssetKind::Crate),
        "npm" => Some(AssetKind::NpmPackage),
        _ => None,
    }
}

/// A range's events, in order, as intervals: each `introduced` opens one,
/// the next `fixed` or `last_affected` closes it. `introduced: "0"` is
/// every version.
fn intervals(range: &OsvRange) -> Vec<Interval> {
    let mut done = Vec::new();
    let mut open: Option<Interval> = None;
    for event in &range.events {
        if let Some(introduced) = &event.introduced {
            done.extend(open.take());
            open = Some(Interval {
                introduced: Some(introduced.clone()).filter(|version| version != "0"),
                ..Interval::default()
            });
        }
        if event.fixed.is_some() || event.last_affected.is_some() {
            let mut closed = open.take().unwrap_or_default();
            closed.fixed.clone_from(&event.fixed);
            closed.last_affected.clone_from(&event.last_affected);
            done.push(closed);
        }
    }
    done.extend(open);
    done
}
