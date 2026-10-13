//! Crates.io registry protocol support
//!
//! Implements the cargo sparse registry protocol for proxying/mirroring crates.

mod handlers;
mod types;

pub use handlers::handle_sparse_index;
pub use types::index_storage_path;

#[cfg(test)]
pub(crate) use handlers::test_override::override_crates_index_base;
