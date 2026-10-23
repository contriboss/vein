mod catalog;
mod cli;
mod gem;
mod health;
mod init;
mod quarantine;
mod server;
mod setup;
mod stats;

use anyhow::Result;

use self::cli::{CatalogCommand, Command, GemCommand, QuarantineCommand};

pub use self::cli::Cli;

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Serve { config } => server::run_server(config),
        Command::Stats { config } => stats::run_stats(config),
        Command::Catalog { action } => match action {
            CatalogCommand::Sync { config } => catalog::run_catalog_sync(config),
        },
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
                gem,
                version,
                platform,
                reason,
            } => quarantine::run_quarantine_approve(
                config,
                &gem,
                &version,
                platform.as_deref(),
                &reason,
            ),
            QuarantineCommand::Block {
                config,
                gem,
                version,
                platform,
                reason,
            } => quarantine::run_quarantine_block(
                config,
                &gem,
                &version,
                platform.as_deref(),
                &reason,
            ),
        },
        Command::Init { output, force } => init::run_init(&output, force),
    }
}
