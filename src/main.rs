mod bin_support;

use anyhow::Result;
use clap::Parser;

use crate::bin_support::{Cli, run};

fn main() -> Result<()> {
    run(Cli::parse())
}
