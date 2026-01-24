//! NPM registry proxy handlers
//!
//! Proxies requests to registry.npmjs.org with caching.
//! Detection is header-based: npm clients send `npm-command` or npm User-Agent.

mod handlers;
mod types;

pub use handlers::{handle_npm_request, is_npm_request};

#[cfg(test)]
pub(crate) use handlers::test_override::override_npm_registry_base;
