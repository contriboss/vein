//! The delay on npm packuments.
//!
//! Every version is dated in the `time` map
//! (`"1.3.0": "2018-04-09T01:10:45.796Z"`). vein asks the registry for the
//! full packument, which carries it; the abbreviated one npm and bun
//! request does not. Per-version manifests (`GET /<name>/<version>`) pass
//! through: clients resolve versions from the packument.

use jiff::Timestamp;
use serde_json::{Map, Value};
use vein_adapter::{AssetKind, CacheBackend, is_semver_prerelease, sort_newest_first};

use super::{Listed, screen};
use crate::config::DelayPolicyConfig;

/// Drops the versions the policy holds or blocks for package `name` from
/// `packument`, and moves dist-tags off them: `latest` to the newest
/// release still visible, any other tag away.
pub async fn filter_packument(
    config: &DelayPolicyConfig,
    index: &CacheBackend,
    name: &str,
    packument: &mut Value,
) {
    let Some(document) = packument.as_object_mut() else {
        return;
    };
    let hidden = hidden_versions(config, index, name, document).await;
    if hidden.is_empty() {
        return;
    }
    remove_versions(document, &hidden);
}

async fn hidden_versions(
    config: &DelayPolicyConfig,
    index: &CacheBackend,
    name: &str,
    document: &Map<String, Value>,
) -> Vec<String> {
    let Some(versions) = document.get("versions").and_then(Value::as_object) else {
        return Vec::new();
    };
    let time = document.get("time").and_then(Value::as_object);
    let listed: Vec<Listed<'_>> = versions
        .keys()
        .map(|version| Listed {
            version,
            platform: None,
            published_at: time
                .and_then(|time| time.get(version))
                .and_then(Value::as_str)
                .and_then(|at| at.parse::<Timestamp>().ok()),
        })
        .collect();
    let Some(verdicts) = screen(config, index, AssetKind::NpmPackage, name, &listed).await else {
        return Vec::new();
    };
    listed
        .iter()
        .zip(verdicts)
        .filter(|(_, verdict)| !verdict.is_visible())
        .map(|(listed, _)| listed.version.to_owned())
        .collect()
}

/// `hidden` out of `versions` and `time`, and no dist-tag left on one.
pub fn remove_versions(document: &mut Map<String, Value>, hidden: &[String]) {
    for field in ["versions", "time"] {
        if let Some(map) = document.get_mut(field).and_then(Value::as_object_mut) {
            map.retain(|version, _| !hidden.contains(version));
        }
    }
    let newest_release = document
        .get("versions")
        .and_then(Value::as_object)
        .and_then(|versions| {
            let mut releases: Vec<&str> = versions
                .keys()
                .map(String::as_str)
                .filter(|version| !is_semver_prerelease(version))
                .collect();
            sort_newest_first(&mut releases, |version| version);
            releases.first().map(|version| (*version).to_owned())
        });
    if let Some(tags) = document.get_mut("dist-tags").and_then(Value::as_object_mut) {
        tags.retain(|tag, version| {
            tag == "latest"
                || !version
                    .as_str()
                    .is_some_and(|v| hidden.iter().any(|h| h == v))
        });
        let latest_hidden = tags
            .get("latest")
            .and_then(Value::as_str)
            .is_some_and(|latest| hidden.iter().any(|h| h == latest));
        if latest_hidden {
            match newest_release {
                Some(release) => {
                    tags.insert("latest".to_owned(), Value::String(release));
                }
                None => {
                    tags.remove("latest");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
