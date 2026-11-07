//! NPM registry proxy handlers
//!
//! Proxies requests to registry.npmjs.org with caching.
//! Requests are told apart in [`crate::proxy::route`].

mod handlers;
mod types;

pub use handlers::handle_npm_request;
pub use types::metadata_storage_path;

#[cfg(test)]
pub(crate) use handlers::test_override::override_npm_registry_base;
