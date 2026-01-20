//! Crates.io registry protocol support
//!
//! Implements the cargo sparse registry protocol for proxying/mirroring crates.

mod handlers;
mod types;

pub use handlers::handle_sparse_index;
