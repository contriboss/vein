#![warn(
    rust_2024_compatibility,
    clippy::all,
    clippy::future_not_send,
    clippy::mod_module_files,
    clippy::needless_pass_by_ref_mut,
    clippy::unused_async
)]

mod bin_support;

use anyhow::Result;
use clap::Parser;

use crate::bin_support::{Cli, run};

fn main() -> Result<()> {
    run(Cli::parse())
}
