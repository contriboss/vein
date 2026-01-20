#![warn(
    rust_2024_compatibility,
    clippy::all,
    clippy::future_not_send,
    clippy::mod_module_files,
    clippy::needless_pass_by_ref_mut,
    clippy::unused_async
)]

pub mod catalog;
pub mod config;
pub mod crates;
pub mod db;
pub mod gem_metadata;
pub mod http_cache;
pub mod proxy;
pub mod quarantine;
pub mod upstream;
