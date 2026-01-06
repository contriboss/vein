//! Vein admin dashboard.

mod app;
mod commands;
mod config;
mod controllers;
mod error;
mod router;
mod ruby;
mod state;
mod utils;
mod views;

use std::time::Duration;

use clap::{Parser, Subcommand};
use rama::{
    graceful::Shutdown,
    http::server::HttpServer,
    layer::ConsumeErrLayer,
    rt::Executor,
    tcp::server::TcpListener,
    tls::rustls::dep::rustls,
    Layer,
};

#[derive(Debug, Parser)]
#[command(author, version, about = "Vein admin dashboard")]
struct Cli {
    /// Path to the admin configuration file
    #[arg(short, long, default_value = "crates/vein-admin/config.toml")]
    config: String,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Start the admin web server
    Serve {
        /// Bind address for the admin server (overrides config)
        #[arg(long)]
        bind: Option<String>,

        /// Port for the admin server (overrides config)
        #[arg(long)]
        port: Option<u16>,
    },
    /// Sync a gem from upstream to local cache
    Sync {
        /// Gem name to sync
        name: String,

        /// Specific version to sync (syncs all if not provided)
        #[arg(short, long)]
        version: Option<String>,

        /// Platform variant (e.g., ruby, java, x86_64-linux)
        #[arg(short, long)]
        platform: Option<String>,
    },
    /// Index Ruby symbols from cached gems
    Index {
        /// Gem name to index
        name: String,

        /// Specific version to index (indexes all cached if not provided)
        #[arg(short, long)]
        version: Option<String>,
    },
    /// Validate binary architecture matches gem platform
    Validate {
        /// Gem name to validate
        name: String,

        /// Specific version to validate (validates all cached if not provided)
        #[arg(short, long)]
        version: Option<String>,
    },
}

async fn run_server(
    cfg: &config::AdminConfig,
    bind: Option<String>,
    port: Option<u16>,
) -> anyhow::Result<()> {
    let state = app::bootstrap(cfg).await?;
    let router = router::build(state);

    let addr = format!(
        "{}:{}",
        bind.unwrap_or_else(|| cfg.server.host.clone()),
        port.unwrap_or(cfg.server.port)
    );

    tracing::info!(%addr, "starting admin server");

    let graceful = Shutdown::default();
    graceful.spawn_task_fn(move |guard| async move {
        let tcp = TcpListener::build().bind(&addr).await.expect("bind tcp");
        let exec = Executor::graceful(guard.clone());
        let service = HttpServer::auto(exec)
            .service(ConsumeErrLayer::default().into_layer(router));
        tcp.serve_graceful(guard, service).await;
    });

    tokio::signal::ctrl_c().await?;
    graceful
        .shutdown_with_limit(Duration::from_secs(30))
        .await?;

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize rustls crypto provider
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let cli = Cli::parse();
    let cfg = config::AdminConfig::load(&cli.config)?;

    tracing_subscriber::fmt()
        .with_env_filter(&cfg.logging.level)
        .init();

    match cli.command {
        Some(Commands::Serve { bind, port }) => {
            run_server(&cfg, bind, port).await?;
        }
        None => {
            run_server(&cfg, None, None).await?;
        }
        Some(Commands::Sync {
            name,
            version,
            platform,
        }) => {
            commands::sync::run(name, version, platform).await?;
        }
        Some(Commands::Index { name, version }) => {
            commands::index::run(name, version).await?;
        }
        Some(Commands::Validate { name, version }) => {
            commands::validate::run(name, version).await?;
        }
    }

    Ok(())
}
