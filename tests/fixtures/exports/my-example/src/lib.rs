/// Says hello.
///
/// At length.
#[inline]
pub fn hello() {}

// A plain comment, not docs.
pub struct World {
    name: String,
}

/** A block doc. */
pub enum Mood {
    Glad,
}

pub mod utils {
    pub fn helper() {}
    pub(crate) fn internal() {}
}

pub mod a {
    pub fn new() {}
}

pub mod b {
    pub fn new() {}
}

pub use std::collections::HashMap;
pub use std::sync::Arc as Shared;

pub(super) struct Hidden;

fn private() {}
