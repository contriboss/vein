use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(author, version, about = "Vein multi-ecosystem package proxy")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Start the Vein proxy server
    Serve {
        /// Path to the configuration file
        #[arg(long, default_value = "vein.toml")]
        config: PathBuf,
    },
    /// Display cache statistics for Vein databases
    Stats {
        /// Path to the configuration file
        #[arg(long, default_value = "vein.toml")]
        config: PathBuf,
    },
    /// Catalogue operations
    Catalog {
        #[command(subcommand)]
        action: CatalogCommand,
    },
    /// Perform a health check against a Vein instance
    Health {
        /// URL of the health endpoint (defaults to local proxy)
        #[arg(long, default_value = "http://127.0.0.1:8346/up")]
        url: String,
        /// Timeout in seconds for the request
        #[arg(long, default_value_t = 5)]
        timeout: u64,
    },
    /// Quarantine management operations
    Quarantine {
        #[command(subcommand)]
        action: QuarantineCommand,
    },
    /// Initialize a new vein configuration file
    Init {
        /// Output path for config file
        #[arg(long, short, default_value = "vein.toml")]
        output: PathBuf,
        /// Overwrite existing config file
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum CatalogCommand {
    /// Sync the upstream gem catalogue immediately
    Sync {
        /// Path to the configuration file
        #[arg(long, default_value = "vein.toml")]
        config: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum QuarantineCommand {
    /// Show quarantine statistics
    Status {
        /// Path to the configuration file
        #[arg(long, default_value = "vein.toml")]
        config: PathBuf,
    },
    /// List versions currently in quarantine
    List {
        /// Path to the configuration file
        #[arg(long, default_value = "vein.toml")]
        config: PathBuf,
        /// Maximum number of entries to show
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
    /// Manually promote all expired quarantines now
    Promote {
        /// Path to the configuration file
        #[arg(long, default_value = "vein.toml")]
        config: PathBuf,
    },
    /// Approve a specific gem version for immediate availability
    Approve {
        /// Path to the configuration file
        #[arg(long, default_value = "vein.toml")]
        config: PathBuf,
        /// Gem name
        gem: String,
        /// Version string
        version: String,
        /// Platform (optional)
        #[arg(long)]
        platform: Option<String>,
        /// Reason for approval
        #[arg(long, default_value = "cli approval")]
        reason: String,
    },
    /// Block a specific gem version (mark as yanked)
    Block {
        /// Path to the configuration file
        #[arg(long, default_value = "vein.toml")]
        config: PathBuf,
        /// Gem name
        gem: String,
        /// Version string
        version: String,
        /// Platform (optional)
        #[arg(long)]
        platform: Option<String>,
        /// Reason for blocking
        #[arg(long, default_value = "cli block")]
        reason: String,
    },
}
