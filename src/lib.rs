#![warn(
    rust_2024_compatibility,
    clippy::all,
    clippy::future_not_send,
    clippy::mod_module_files,
    clippy::needless_pass_by_ref_mut,
    clippy::unused_async
)]

// Library target to enable unit testing of internal modules
pub mod catalog;
pub mod config;
pub mod db;
pub mod gem_metadata;
pub mod hotcache;
pub mod proxy;
pub mod upstream;

pub use vein_adapter::*;
