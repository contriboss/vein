mod catalog;
mod cli;
mod health;
mod init;
mod quarantine;
mod server;
mod setup;
mod stats;

use anyhow::Result;

use self::cli::{CatalogCommand, Command, QuarantineCommand};

pub(crate) use self::cli::Cli;

pub(crate) fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Serve { config } => server::run_server(config),
        Command::Stats { config } => stats::run_stats(config),
        Command::Catalog { action } => match action {
            CatalogCommand::Sync { config } => catalog::run_catalog_sync(config),
        },
        Command::Health { url, timeout } => health::run_health(url, timeout),
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
            } => quarantine::run_quarantine_approve(config, gem, version, platform, reason),
            QuarantineCommand::Block {
                config,
                gem,
                version,
                platform,
                reason,
            } => quarantine::run_quarantine_block(config, gem, version, platform, reason),
        },
        Command::Init { output, force } => init::run_init(output, force),
    }
}
