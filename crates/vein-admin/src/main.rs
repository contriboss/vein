mod app;
mod commands;
mod controllers;
mod initializers;
mod ruby;
mod state;
mod views;

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use loco_rs::{
    boot::{self, ServeParams, StartMode},
    environment::Environment,
};

use crate::app::App;

#[derive(Debug, Parser)]
#[command(author, version, about = "Vein admin dashboard")]
struct Cli {
    /// Environment to run in (development, production, test)
    #[arg(short, long, default_value = "development")]
    environment: String,

    /// Path to the Vein configuration file
    #[arg(long)]
    config: Option<PathBuf>,

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

#[tokio::main]
async fn main() -> loco_rs::Result<()> {
    // Initialize rustls crypto provider
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let cli = Cli::parse();

    // Parse environment
    let environment: Environment = cli
        .environment
        .parse()
        .map_err(|_| loco_rs::Error::Message("Invalid environment".to_string()))?;

    // Override vein config path if provided via CLI
    if let Some(config_path) = cli.config {
        unsafe {
            std::env::set_var("VEIN_CONFIG_PATH", config_path.display().to_string());
        }
    }

    match cli.command {
        Some(Commands::Serve { bind, port }) => {
            // Create Loco app using standard boot process
            let config_path = std::path::Path::new("crates/vein-admin/config");
            let app = loco_rs::boot::create_app::<App, migration::Migrator>(
                StartMode::ServerOnly,
                &environment,
                loco_rs::config::Config::from_folder(&environment, config_path)?,
            )
            .await?;

            // Get binding and port from config or CLI overrides
            let binding = bind.unwrap_or_else(|| app.app_context.config.server.binding.clone());
            let port = port
                .map(|p| p as i32)
                .unwrap_or(app.app_context.config.server.port);

            let serve_params = ServeParams { port, binding };

            // Start the server
            boot::start::<App>(app, serve_params, false).await?;
        }
        Some(Commands::Sync {
            name,
            version,
            platform,
        }) => {
            commands::sync::run(name, version, platform)
                .await
                .map_err(|e| loco_rs::Error::Message(e.to_string()))?;
        }
        Some(Commands::Index { name, version }) => {
            commands::index::run(name, version)
                .await
                .map_err(|e| loco_rs::Error::Message(e.to_string()))?;
        }
        Some(Commands::Validate { name, version }) => {
            commands::validate::run(name, version)
                .await
                .map_err(|e| loco_rs::Error::Message(e.to_string()))?;
        }
        None => {
            // Default to serve if no subcommand provided
            let config_path = std::path::Path::new("crates/vein-admin/config");
            let app = loco_rs::boot::create_app::<App, migration::Migrator>(
                StartMode::ServerOnly,
                &environment,
                loco_rs::config::Config::from_folder(&environment, config_path)?,
            )
            .await?;

            let binding = app.app_context.config.server.binding.clone();
            let port = app.app_context.config.server.port;
            let serve_params = ServeParams { port, binding };

            boot::start::<App>(app, serve_params, false).await?;
        }
    }

    Ok(())
}
