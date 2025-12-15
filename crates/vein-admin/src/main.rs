mod app;
mod controllers;
mod initializers;
mod ruby;
mod state;
mod views;

use std::path::PathBuf;

use clap::Parser;
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

    /// Bind address for the admin server (overrides config)
    #[arg(long)]
    bind: Option<String>,

    /// Port for the admin server (overrides config)
    #[arg(long)]
    port: Option<u16>,

    /// Path to the Vein configuration file (passed via settings in config YAML)
    #[arg(long)]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> loco_rs::Result<()> {
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

    // Create Loco app using standard boot process
    let config_path = std::path::Path::new("crates/vein-admin/config");
    let app = loco_rs::boot::create_app::<App, migration::Migrator>(
        StartMode::ServerOnly,
        &environment,
        loco_rs::config::Config::from_folder(&environment, config_path)?,
    )
    .await?;

    // Get binding and port from config or CLI overrides
    let binding = cli
        .bind
        .unwrap_or_else(|| app.app_context.config.server.binding.clone());

    let port = cli
        .port
        .map(|p| p as i32)
        .unwrap_or(app.app_context.config.server.port);

    let serve_params = ServeParams { port, binding };

    // Start the server
    boot::start::<App>(app, serve_params, false).await?;

    Ok(())
}
