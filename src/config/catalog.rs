//! Which registries' full name lists vein keeps, so search still answers
//! when a registry is out of reach.

use serde::Deserialize;
use vein_adapter::AssetKind;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CatalogConfig {
    /// `gem`: rubygems.org's `names.gz`, under 3 MB. `crate`: crates.io's
    /// ~360k names, an hour of paging at its 1 request/second crawler
    /// limit the first time. `npm`: ~4.5M names, ~450 MB to replicate the
    /// first time. Each is kept up to date every 6 hours after that.
    pub registries: Vec<AssetKind>,
}

impl Default for CatalogConfig {
    fn default() -> Self {
        Self {
            registries: vec![AssetKind::Gem],
        }
    }
}
