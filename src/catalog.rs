//! Every name each registry publishes, kept so search answers offline.
//!
//! Sources: rubygems.org's `names.gz`, crates.io's API, npm's replication
//! endpoint. Which ones is `[catalog] registries`; [`crate::jobs`] runs the
//! syncs every 6 hours.

#[cfg(feature = "crates")]
mod crates;
#[cfg(feature = "gems")]
mod gems;
#[cfg(feature = "npm")]
mod npm;

use std::time::Duration;

use anyhow::Result;
use rama::{
    Layer, Service,
    error::{ErrorExt as _, extra::OpaqueError},
    http::{
        HeaderValue, Request, Response, client::EasyHttpWebClient,
        layer::required_header::AddRequiredRequestHeadersLayer,
    },
    layer::{MapErrLayer, TimeoutLayer},
};
use vein_adapter::{AssetKind, CacheBackend};

/// crates.io's crawler policy: one request a second.
#[cfg(feature = "crates")]
const CRATES_PACE: Duration = Duration::from_secs(1);
/// npm's replica states no limit; vein keeps a polite gap anyway.
#[cfg(feature = "npm")]
const NPM_PACE: Duration = Duration::from_millis(250);

/// crates.io asks crawlers to say who they are and where to read about them.
const USER_AGENT: &str = concat!(
    "vein/",
    env!("CARGO_PKG_VERSION"),
    " (catalog sync; +https://github.com/contriboss/vein)"
);

/// Syncs registry `kind`'s names once; how many changed, `None` when the
/// registry said nothing had.
pub async fn sync_once(index: &CacheBackend, kind: AssetKind) -> Result<Option<usize>> {
    let client = build_client();
    match kind {
        #[cfg(feature = "gems")]
        AssetKind::Gem | AssetKind::Spec => gems::sync(index, &client).await,
        #[cfg(feature = "crates")]
        AssetKind::Crate => crates::sync(index, &client, crates::API, CRATES_PACE)
            .await
            .map(Some),
        #[cfg(feature = "npm")]
        AssetKind::NpmPackage => npm::sync(index, &client, npm::REPLICATE, NPM_PACE)
            .await
            .map(Some),
        #[allow(unreachable_patterns)]
        kind => Err(crate::registry::NotBuilt(kind).into()),
    }
}

fn build_client() -> impl Service<Request, Output = Response, Error = OpaqueError> {
    (
        MapErrLayer::new(|e: rama::error::BoxError| e.into_opaque_error()),
        TimeoutLayer::new(Duration::from_secs(60)),
        AddRequiredRequestHeadersLayer::new()
            .with_user_agent_header_value(HeaderValue::from_static(USER_AGENT)),
    )
        .into_layer(EasyHttpWebClient::default())
}
