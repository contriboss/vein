// Unexercised `pub` items are expected in a library compiled as a test binary.
#![cfg_attr(test, allow(dead_code_pub_in_binary))]

pub mod prelude;

#[cfg(feature = "admin")]
pub mod admin;
pub mod catalog;
pub mod code_context;
pub mod config;
#[cfg(feature = "crates")]
pub mod crates;
pub mod db;
pub mod delay;
pub mod docs;
#[cfg(feature = "gems")]
pub mod gem_metadata;
#[cfg(feature = "gems")]
pub mod gem_sync;
pub mod http_cache;
pub mod jobs;
pub mod loopback;
#[cfg(feature = "npm")]
pub mod npm;
pub mod parse;
pub mod proxy;
pub mod registry;
pub mod search;
pub mod untrusted;
pub mod upstream;
pub mod util;

#[cfg(test)]
mod test_support;
