//! NPM registry proxy handlers
//!
//! Proxies requests to registry.npmjs.org with caching.
//! Detection is header-based: npm clients send `npm-command` or npm User-Agent.

mod handlers;
mod types;

pub use handlers::{handle_npm_request, is_npm_request};
