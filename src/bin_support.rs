mod catalog;
mod cli;
#[cfg(feature = "gems")]
mod gem;
mod health;
mod init;
mod quarantine;
mod server;
mod setup;
mod stats;

use anyhow::Result;
use vein_adapter::AssetKey;

#[cfg(feature = "gems")]
use self::cli::GemCommand;
use self::cli::{CatalogCommand, Command, QuarantineCommand};

pub use self::cli::Cli;

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Serve { config } => server::run_server(config),
        Command::Stats { config } => stats::run_stats(config),
        Command::Catalog { action } => match action {
            CatalogCommand::Sync { config, registry } => {
                catalog::run_catalog_sync(config, registry)
            }
        },
        #[cfg(feature = "gems")]
        Command::Gem { action } => match action {
            GemCommand::Sync {
                config,
                name,
                version,
                platform,
                reanalyze,
            } => gem::run_gem_sync(
                config,
                &name,
                version.as_deref(),
                platform.as_deref(),
                reanalyze,
            ),
        },
        Command::Health { url, timeout } => health::run_health(&url, timeout),
        Command::Quarantine { action } => match action {
            QuarantineCommand::Status { config } => quarantine::run_quarantine_status(config),
            QuarantineCommand::List { config, limit } => {
                quarantine::run_quarantine_list(config, limit)
            }
            QuarantineCommand::Promote { config } => quarantine::run_quarantine_promote(config),
            QuarantineCommand::Approve {
                config,
                registry,
                name,
                version,
                platform,
                reason,
            } => quarantine::run_quarantine_approve(
                config,
                &AssetKey {
                    kind: vein::registry::require(registry)?,
                    name: &name,
                    version: &version,
                    platform: platform.as_deref(),
                },
                &reason,
            ),
            QuarantineCommand::Block {
                config,
                registry,
                name,
                version,
                platform,
                reason,
            } => quarantine::run_quarantine_block(
                config,
                &AssetKey {
                    kind: vein::registry::require(registry)?,
                    name: &name,
                    version: &version,
                    platform: platform.as_deref(),
                },
                &reason,
            ),
        },
        Command::Init { output, force } => init::run_init(&output, force),
    }
}
