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

/// What a crawl waits between pages, and whether its job wants it to stop.
#[cfg(any(feature = "crates", feature = "npm"))]
#[derive(Clone, Copy)]
pub struct Pacing<'a> {
    pace: Duration,
    stop: &'a (dyn Fn() -> bool + Sync),
}

#[cfg(any(feature = "crates", feature = "npm"))]
impl<'a> Pacing<'a> {
    const fn new(pace: Duration, stop: &'a (dyn Fn() -> bool + Sync)) -> Self {
        Self { pace, stop }
    }

    /// Called once a page's checkpoint is saved: waits the pace, or fails
    /// with [`Stopped`] when the job asked to stop.
    async fn next_page(self) -> Result<()> {
        if (self.stop)() {
            return Err(Stopped.into());
        }
        tokio::time::sleep(self.pace).await;
        Ok(())
    }
}

/// A crawl stopped at a checkpoint because its job was cancelled.
///
/// Vein shutting down or the job's deadline cancels it. An error, not a
/// short count: the sync must not record itself done, and the retry resumes.
#[cfg(any(feature = "crates", feature = "npm"))]
#[derive(Debug)]
pub struct Stopped;

#[cfg(any(feature = "crates", feature = "npm"))]
impl std::fmt::Display for Stopped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("catalog sync stopped at a checkpoint")
    }
}

#[cfg(any(feature = "crates", feature = "npm"))]
impl std::error::Error for Stopped {}

/// Syncs registry `kind`'s names once; how many changed, `None` when the
/// registry said nothing had. Crawls check `stop` between pages.
#[cfg_attr(
    not(any(feature = "crates", feature = "npm")),
    expect(unused_variables, reason = "only paged crawls can stop")
)]
pub async fn sync_once(
    index: &CacheBackend,
    kind: AssetKind,
    stop: &(dyn Fn() -> bool + Sync),
) -> Result<Option<usize>> {
    let client = build_client();
    match kind {
        #[cfg(feature = "gems")]
        AssetKind::Gem | AssetKind::Spec => gems::sync(index, &client).await,
        #[cfg(feature = "crates")]
        AssetKind::Crate => {
            crates::sync(index, &client, crates::API, Pacing::new(CRATES_PACE, stop))
                .await
                .map(Some)
        }
        #[cfg(feature = "npm")]
        AssetKind::NpmPackage => {
            npm::sync(index, &client, npm::REPLICATE, Pacing::new(NPM_PACE, stop))
                .await
                .map(Some)
        }
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
