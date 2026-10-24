//! Code context for the MCP's tools, from a provider indexing public
//! open-source code and docs.
//!
//! Search inside a package or repository, read and grep its files, examples
//! from other projects, research with sources. A [`Provider`] declares its
//! tools and answers calls to them; the MCP offers each under the
//! provider's own name, so the follow-ups a result suggests (`read
//! target=... path=...`) name tools agents can call. `GitHits` is the first;
//! a provider over vein's own cache can come next.

#[cfg(feature = "githits")]
pub mod githits;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use serde_json::{Map, Value};

use crate::config::code_context::CodeContextConfig;
#[cfg(feature = "githits")]
use crate::config::code_context::Provider as Configured;

/// What a provider's call returns.
pub type Pending<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

/// A source of code context tools.
pub trait Provider: Send + Sync {
    /// Its name, as agents read it: `GitHits`.
    fn name(&self) -> &'static str;

    /// What it is, for the MCP's instructions: what it indexes, what its
    /// tools do, how they name a package or repository.
    fn about(&self) -> &'static str;

    /// Whether its queries leave this network (a hosted service), so agents
    /// are told before they send one.
    fn remote(&self) -> bool;

    /// The tools it offers now.
    fn tools(&self) -> Pending<'_, Vec<ToolSpec>>;

    /// Calls tool `name` with `arguments`, as its input schema describes them.
    fn call<'a>(&'a self, name: &'a str, arguments: Map<String, Value>) -> Pending<'a, Answer>;
}

/// One tool a provider offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSpec {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    /// A JSON Schema object.
    pub input_schema: Value,
    /// It changes nothing anywhere.
    pub read_only: bool,
    /// It reaches beyond what the provider holds (the open web, live
    /// registries).
    pub open_world: bool,
}

/// A tool's answer: text for the agent, and whether it is an error the
/// agent should read (a bad target, an unknown path) rather than a result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub text: String,
    pub is_error: bool,
}

/// The provider `config` names, if any. Fails when it cannot be set up (no
/// token); a config naming one this build lacks is refused earlier, by
/// [`crate::config::Config::validate`].
pub fn from_config(config: &CodeContextConfig) -> Result<Option<Arc<dyn Provider>>> {
    match config.provider {
        None => Ok(None),
        #[cfg(feature = "githits")]
        Some(Configured::GitHits) => Ok(Some(Arc::new(githits::GitHits::new(&config.githits)?))),
        #[allow(unreachable_patterns)]
        Some(provider) => anyhow::bail!(
            "this vein was built without the `{}` feature",
            provider.feature()
        ),
    }
}

#[cfg(test)]
mod tests;
