//! Who is calling `/mcp`, once write tools are on. The write token's holder
//! is the admin; the read token's, when one is set, an agent; a caller
//! without a token is an anonymous reader, admitted only where reads need
//! no token (loopback, or trusted networks). Any other token is refused.

use mcp_host::prelude::VisibilityContext;
use mcp_host::server::principal::AuthenticatedPrincipal;
use mcp_host::transport::http::{
    HttpAuthFuture, HttpAuthOutcome, HttpAuthRequest, HttpAuthenticator, HttpTransportError,
};

/// The principals' `source`: vein told them apart, not mcp-host.
const SOURCE: &str = "vein";
/// Who may call the write tools.
pub const ADMIN: &str = "admin";

/// The tokens callers are told apart by.
pub struct Callers {
    /// `VEIN_ADMIN_MCP_TOKEN`: required of every caller when set.
    read: Option<String>,
    /// `VEIN_ADMIN_MCP_WRITE_TOKEN`: the admin's.
    write: String,
}

impl std::fmt::Debug for Callers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the tokens themselves.
        f.debug_struct("Callers")
            .field("read", &self.read.is_some())
            .finish_non_exhaustive()
    }
}

impl Callers {
    pub const fn new(read: Option<String>, write: String) -> Self {
        Self { read, write }
    }

    /// The caller behind an `Authorization` header, if it may come in.
    fn identify(
        &self,
        authorization: Option<&str>,
    ) -> Result<AuthenticatedPrincipal, HttpTransportError> {
        let token = authorization
            .map(|value| {
                value
                    .strip_prefix("Bearer ")
                    .ok_or(HttpTransportError::Unauthorized)
            })
            .transpose()?;
        let subject = match token {
            Some(token) if same(token, &self.write) => ADMIN,
            Some(token) if self.read.as_deref().is_some_and(|read| same(token, read)) => "agent",
            None if self.read.is_none() => "anonymous",
            _ => return Err(HttpTransportError::Unauthorized),
        };
        Ok(AuthenticatedPrincipal::new(SOURCE, subject))
    }
}

impl HttpAuthenticator for Callers {
    fn authenticate<'a>(&'a self, request: HttpAuthRequest<'a>) -> HttpAuthFuture<'a> {
        let outcome = self
            .identify(request.authorization)
            .map(HttpAuthOutcome::new);
        Box::pin(async move { outcome })
    }
}

/// Whether the session's caller presented the write token.
pub fn is_admin(ctx: &VisibilityContext<'_>) -> bool {
    ctx.authenticated_principal()
        .is_some_and(|principal| principal.source == SOURCE && principal.subject == ADMIN)
}

/// Compares tokens in time that depends on their lengths only.
fn same(given: &str, expected: &str) -> bool {
    given.len() == expected.len()
        && given
            .bytes()
            .zip(expected.bytes())
            .fold(0_u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}

#[cfg(test)]
mod tests;
