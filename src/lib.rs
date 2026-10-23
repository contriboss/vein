// Unexercised `pub` items are expected in a library compiled as a test binary.
#![cfg_attr(test, allow(dead_code_pub_in_binary))]

#[cfg(feature = "admin")]
pub mod admin;
pub mod catalog;
pub mod config;
pub mod crates;
pub mod db;
pub mod delay;
pub mod gem_metadata;
pub mod gem_sync;
pub mod http_cache;
pub mod jobs;
pub mod npm;
pub mod proxy;
pub mod search;
pub mod upstream;
pub mod util;

#[cfg(test)]
mod test_support;
