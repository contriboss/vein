//! `[admin] trusted_networks`: which peers may use the admin listener.
//!
//! One list gates both the dashboard (its quarantine approve/block included)
//! and `/mcp`, where mcp-host checks the same networks and admits them
//! without a token.

use rama::telemetry::tracing;
use std::{net::IpAddr, sync::Arc};

use anyhow::{Context, Result};
use mcp_host::transport::http::IpNet;
use rama::{
    Service,
    extensions::ExtensionsRef,
    http::{Body, Request, Response, StatusCode},
    net::stream::SocketInfo,
};

use crate::config::AdminConfig;

/// The configured networks, plus loopback so tools on the host itself keep
/// working; empty when none are configured (no gate).
///
/// # Errors
///
/// When an entry is not a CIDR block or an address.
pub fn trusted_networks(admin: &AdminConfig) -> Result<Vec<IpNet>> {
    if admin.trusted_networks.is_empty() {
        return Ok(Vec::new());
    }
    let mut networks = admin
        .trusted_networks
        .iter()
        .map(|entry| parse_network(entry))
        .collect::<Result<Vec<_>>>()?;
    for loopback in ["127.0.0.0/8", "::1/128"] {
        networks.push(loopback.parse().expect("loopback CIDR literals parse"));
    }
    Ok(networks)
}

/// `192.168.1.0/24`, or a single address (`192.168.1.42`) as its host route.
fn parse_network(entry: &str) -> Result<IpNet> {
    let entry = entry.trim();
    entry
        .parse::<IpNet>()
        .or_else(|_| entry.parse::<IpAddr>().map(IpNet::from))
        .with_context(|| {
            format!("[admin] trusted_networks: `{entry}` is not a CIDR block or an address")
        })
}

/// Whether `peer` may reach the admin listener. No networks: everyone (the
/// listener is loopback, or the operator chose an open bind). With networks,
/// an unknown peer is refused; IPv4-mapped IPv6 peers match as IPv4.
fn admitted(peer: Option<IpAddr>, networks: &[IpNet]) -> bool {
    networks.is_empty()
        || peer
            .map(|ip| ip.to_canonical())
            .is_some_and(|ip| networks.iter().any(|net| net.contains(&ip)))
}

/// Refuses peers outside `networks` with `403` before `inner` sees them.
pub struct TrustedPeers<S> {
    inner: S,
    networks: Arc<[IpNet]>,
}

impl<S> TrustedPeers<S> {
    pub fn new(inner: S, networks: Vec<IpNet>) -> Self {
        Self {
            inner,
            networks: networks.into(),
        }
    }
}

impl<S> Service<Request<Body>> for TrustedPeers<S>
where
    S: Service<Request<Body>, Output = Response<Body>>,
{
    type Output = Response<Body>;
    type Error = S::Error;

    async fn serve(&self, req: Request<Body>) -> Result<Self::Output, Self::Error> {
        let peer = req
            .extensions()
            .get_ref::<SocketInfo>()
            .map(|info| info.peer_addr().into_std().ip());
        if admitted(peer, &self.networks) {
            return self.inner.serve(req).await;
        }
        tracing::warn!(?peer, "admin request from outside [admin] trusted_networks");
        let mut forbidden = Response::new(Body::from("forbidden\n"));
        *forbidden.status_mut() = StatusCode::FORBIDDEN;
        Ok(forbidden)
    }
}

#[cfg(test)]
mod tests;
