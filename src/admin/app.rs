//! Bootstrap for the admin dashboard: embedded templates and shared state.

use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
#[cfg(feature = "gems")]
use rama::telemetry::tracing::warn;
use rama::utils::include_dir::{Dir, include_dir};
use tera::Tera;
use vein_adapter::{CacheBackend, FilesystemStorage};

#[cfg(feature = "gems")]
use super::ruby;
use super::state::{AdminResources, AdminState};
use crate::config::Config;

/// Tera templates, embedded at build time; names are paths under `views/`.
const TEMPLATES: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/assets/admin/views");

/// The admin state over the proxy's own config, cache, and storage.
#[cfg_attr(
    not(feature = "gems"),
    expect(
        clippy::unused_async,
        reason = "only gem builds fetch Ruby's release status"
    )
)]
pub async fn bootstrap(
    config: Arc<Config>,
    cache: Arc<CacheBackend>,
    storage: Arc<FilesystemStorage>,
) -> Result<AdminState> {
    let tera = load_templates()?;
    let code_context = crate::code_context::from_config(&config.code_context)?;
    let resources = AdminResources::new(config, cache, storage);
    let resources = match code_context {
        Some(provider) => resources.with_code_context(provider),
        None => resources,
    };
    #[cfg(feature = "gems")]
    let resources = resources.with_ruby_status(load_ruby_status().await);
    Ok(AdminState::new(resources, tera))
}

pub(super) fn load_templates() -> Result<Arc<Tera>> {
    let mut tera = Tera::new();
    register_extensions(&mut tera);
    // Which registries the pages show: `{% if serves.npm %}`.
    tera.global_context().insert(
        "serves",
        &serde_json::json!({
            "gems": crate::registry::built(vein_adapter::AssetKind::Gem),
            "crates": crate::registry::built(vein_adapter::AssetKind::Crate),
            "npm": crate::registry::built(vein_adapter::AssetKind::NpmPackage),
        }),
    );
    let mut templates = Vec::new();
    collect_templates(&TEMPLATES, &mut templates)?;
    tera.add_raw_templates(templates)
        .context("loading admin templates")?;
    Ok(Arc::new(tera))
}

fn collect_templates<'a>(dir: &Dir<'a>, out: &mut Vec<(String, &'a str)>) -> Result<()> {
    for file in dir.files() {
        let name = file
            .path()
            .to_str()
            .ok_or_else(|| anyhow!("template path {} is not UTF-8", file.path().display()))?;
        let source = file
            .contents_utf8()
            .ok_or_else(|| anyhow!("template {name} is not UTF-8"))?;
        out.push((name.to_owned(), source));
    }
    for sub in dir.dirs() {
        collect_templates(sub, out)?;
    }
    Ok(())
}

/// Percent-encode everything except RFC 3986 unreserved characters.
const URLENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

#[must_use]
pub fn urlencode(s: &str) -> String {
    utf8_percent_encode(s, URLENCODE_SET).to_string()
}

// Tera 2 dropped the built-in `urlencode` filter; templates still rely on it.
fn register_extensions(tera: &mut Tera) {
    tera.register_filter("urlencode", |s: &str, _: tera::Kwargs, _: &tera::State| {
        urlencode(s)
    });
}

#[cfg(feature = "gems")]
async fn load_ruby_status() -> Arc<ruby::RubyStatus> {
    match ruby::fetch_ruby_status().await {
        Ok(status) => Arc::new(status),
        Err(err) => {
            warn!(error = %err, "failed to fetch ruby status");
            Arc::new(ruby::RubyStatus::default())
        }
    }
}

#[cfg(test)]
mod tests;
