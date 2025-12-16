#![warn(
    rust_2024_compatibility,
    clippy::all,
    clippy::future_not_send,
    clippy::mod_module_files,
    clippy::needless_pass_by_ref_mut,
    clippy::unused_async
)]

mod db;
mod gem_metadata;
mod hotcache;
mod proxy;
mod upstream;

// Use config from library to avoid type conflicts with quarantine module
use vein::config;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use opentelemetry::{KeyValue, global, trace::TracerProvider};
use opentelemetry_sdk::{resource::Resource, trace as sdktrace};
use rama::{
    Layer as RamaLayer,
    graceful::Shutdown,
    http::{layer::trace::TraceLayer, server::HttpServer},
    layer::ConsumeErrLayer,
    rt::Executor,
    tcp::server::TcpListener,
};
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing_subscriber::{
    layer::{Layer, SubscriberExt},
    util::SubscriberInitExt,
};
use vein_adapter::{CacheBackend, FilesystemStorage};

use config::{Config, DatabaseBackend};
use crate::db::connect_cache_backend;
use crate::hotcache::HotCache;
use crate::proxy::VeinProxy;
use vein::{catalog, quarantine};

#[derive(Debug, Parser)]
#[command(author, version, about = "Vein RubyGems mirror server")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
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
    /// Cache maintenance operations
    Cache {
        #[command(subcommand)]
        action: CacheCommand,
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
}

#[derive(Debug, Subcommand)]
enum CacheCommand {
    /// Refresh the hot cache from the SQLite index
    Refresh {
        /// Path to the configuration file
        #[arg(long, default_value = "vein.toml")]
        config: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum CatalogCommand {
    /// Sync the upstream gem catalogue immediately
    Sync {
        /// Path to the configuration file
        #[arg(long, default_value = "vein.toml")]
        config: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum QuarantineCommand {
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve { config } => run_server(config),
        Command::Stats { config } => run_stats(config),
        Command::Cache { action } => match action {
            CacheCommand::Refresh { config } => run_cache_refresh(config),
        },
        Command::Catalog { action } => match action {
            CatalogCommand::Sync { config } => run_catalog_sync(config),
        },
        Command::Health { url, timeout } => run_health(url, timeout),
        Command::Quarantine { action } => match action {
            QuarantineCommand::Status { config } => run_quarantine_status(config),
            QuarantineCommand::List { config, limit } => run_quarantine_list(config, limit),
            QuarantineCommand::Promote { config } => run_quarantine_promote(config),
            QuarantineCommand::Approve {
                config,
                gem,
                version,
                platform,
                reason,
            } => run_quarantine_approve(config, gem, version, platform, reason),
            QuarantineCommand::Block {
                config,
                gem,
                version,
                platform,
                reason,
            } => run_quarantine_block(config, gem, version, platform, reason),
        },
    }
}

#[allow(unreachable_code)]
fn run_server(config_path: PathBuf) -> Result<()> {
    let config = Arc::new(Config::load(Some(config_path)).context("loading configuration")?);
    config.validate().context("validating configuration")?;
    init_tracing(&config)?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("constructing setup runtime")?;

    let storage = Arc::new(FilesystemStorage::new(config.storage.path.clone()));
    rt.block_on(storage.prepare())
        .context("preparing storage directory")?;

    let (index, backend_kind) = rt
        .block_on(connect_cache_backend(config.as_ref()))
        .context("connecting to cache index")?;

    // Ensure quarantine tables exist if enabled
    rt.block_on(quarantine::ensure_tables(index.as_ref(), &config.delay_policy))
        .context("initializing quarantine tables")?;

    let hot_cache_path = hot_cache_path(config.as_ref(), &backend_kind);
    let hot_cache =
        HotCache::open_with_config(&hot_cache_path, config.hotcache.reliability.clone())
            .context("opening hot cache")?;
    match hot_cache.stats() {
        Ok(stats) => tracing::info!(
            total_entries = stats.total_entries,
            cached_gems = stats.cached_gems,
            latest_versions = stats.latest_versions,
            "hot cache initialized"
        ),
        Err(err) => tracing::warn!(error = %err, "failed to read hot cache stats"),
    }

    // Set up hot cache refresh scheduler if enabled
    if !config.hotcache.refresh_schedule.is_empty() {
        let hot_cache_clone = hot_cache.clone();
        let index_clone = index.clone();
        let schedule = config.hotcache.refresh_schedule.clone();

        // Spawn the scheduler on a dedicated long-lived runtime thread
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create scheduler runtime");

            rt.block_on(async {
                let sched = JobScheduler::new()
                    .await
                    .expect("Failed to create job scheduler");

                let job = Job::new_async(schedule.as_str(), move |_uuid, _l| {
                    let hot_cache = hot_cache_clone.clone();
                    let index = index_clone.clone();
                    Box::pin(async move {
                        tracing::info!("Starting hot cache refresh");
                        if let Err(err) = hot_cache.refresh_from_index(index.as_ref()).await {
                            tracing::error!(error = %err, "Hot cache refresh failed");
                        }
                    })
                })
                .expect("Failed to create refresh job");

                sched
                    .add(job)
                    .await
                    .expect("Failed to add refresh job to scheduler");

                sched.start().await.expect("Failed to start job scheduler");

                tracing::info!(schedule = %schedule, "Hot cache refresh scheduler started");

                // Keep the scheduler runtime alive forever
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
                }
            });
        });
    }

    // Start quarantine promotion scheduler if enabled
    quarantine::spawn_promotion_scheduler(&config.delay_policy, index.clone(), None);

    drop(rt);

    let proxy = VeinProxy::new(config.clone(), storage, index.clone(), hot_cache)
        .context("creating proxy service")?;

    let rt_server = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(config.server.workers)
        .enable_all()
        .build()
        .context("constructing server runtime")?;

    rt_server.block_on(async move {
        let graceful = Shutdown::default();
        let addr = format!("{}:{}", config.server.host, config.server.port);

        tracing::info!(%addr, "starting Rama HTTP server");

        graceful.spawn_task_fn(move |guard| {
            let proxy = proxy.clone();
            let addr = addr.clone();
            async move {
                let tcp_service = TcpListener::build()
                    .bind(addr)
                    .await
                    .expect("bind tcp proxy");

                let exec = Executor::graceful(guard.clone());
                let http_service = HttpServer::auto(exec).service(
                    (TraceLayer::new_for_http(), ConsumeErrLayer::default()).into_layer(proxy),
                );

                tcp_service.serve_graceful(guard, http_service).await;
            }
        });

        // Wait for ctrl+c to initiate graceful shutdown
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for shutdown signal");

        graceful
            .shutdown_with_limit(Duration::from_secs(30))
            .await?;

        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}

fn run_stats(config_path: PathBuf) -> Result<()> {
    let config = Arc::new(Config::load(Some(config_path)).context("loading configuration")?);
    init_tracing(&config)?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("constructing stats runtime")?;

    let (index, backend_kind) = rt
        .block_on(connect_cache_backend(config.as_ref()))
        .context("connecting to cache index")?;
    let index_stats = rt
        .block_on(index.stats())
        .context("collecting cache stats")?;

    let hot_cache_path = hot_cache_path(config.as_ref(), &backend_kind);
    let hot_cache =
        HotCache::open_with_config(&hot_cache_path, config.hotcache.reliability.clone())
            .context("opening hot cache")?;
    let hot_stats = hot_cache.stats().context("collecting hot cache stats")?;

    drop(rt);

    match backend_kind {
        DatabaseBackend::Sqlite { path } => {
            println!("SQLite cache: {}", path.display());
        }
        DatabaseBackend::Postgres { url, .. } => {
            println!("PostgreSQL cache: {}", url);
        }
    }
    println!("  total assets: {}", index_stats.total_assets);
    println!("  gem assets: {}", index_stats.gem_assets);
    println!("  gemspec assets: {}", index_stats.spec_assets);
    println!("  unique gems: {}", index_stats.unique_gems);
    println!(
        "  total size: {}",
        format_bytes(index_stats.total_size_bytes)
    );
    if let Some(last) = index_stats.last_accessed {
        println!("  last access: {}", last);
    }

    println!("\nHot cache: {}", hot_cache_path.display());
    println!("  entries: {}", hot_stats.total_entries);
    println!("  cached gems: {}", hot_stats.cached_gems);
    println!("  latest markers: {}", hot_stats.latest_versions);

    Ok(())
}

fn run_health(url: String, timeout: u64) -> Result<()> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout))
        .build()
        .context("building health check client")?;

    let response = client
        .get(&url)
        .send()
        .context("sending health check request")?;

    if response.status().is_success() {
        println!("Vein healthy: {}", response.status());
        Ok(())
    } else {
        bail!("health endpoint returned status {}", response.status());
    }
}

fn init_tracing(config: &Config) -> Result<()> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .or_else(|_| tracing_subscriber::EnvFilter::try_new(&config.logging.level))
        .context("building log filter")?;

    let fmt_layer = if config.logging.json {
        tracing_subscriber::fmt::layer()
            .json()
            .flatten_event(true)
            .with_target(false)
            .boxed()
    } else {
        tracing_subscriber::fmt::layer().with_target(false).boxed()
    };

    let registry = tracing_subscriber::registry().with(filter).with(fmt_layer);

    if let Ok(endpoint) = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        use opentelemetry_otlp::WithExportConfig;

        let resource = Resource::builder_empty()
            .with_attributes([
                KeyValue::new("service.name", "vein"),
                KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
            ])
            .build();

        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_endpoint(endpoint)
            .build()?;

        let provider = sdktrace::SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .with_resource(resource)
            .build();

        let tracer = provider.tracer("vein");
        global::set_tracer_provider(provider);

        registry
            .with(tracing_opentelemetry::layer().with_tracer(tracer))
            .try_init()?;
    } else {
        registry.try_init()?;
    }
    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "0 B".to_string();
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{:.2} {}", value, UNITS[unit])
    }
}

fn hot_cache_path(config: &Config, backend: &DatabaseBackend) -> PathBuf {
    match backend {
        DatabaseBackend::Sqlite { path } => {
            let mut path = path.clone();
            path.set_extension("redb");
            path
        }
        DatabaseBackend::Postgres { .. } => {
            let mut path = config.database.path.clone();
            path.set_extension("redb");
            path
        }
    }
}

fn run_cache_refresh(config_path: PathBuf) -> Result<()> {
    let config = Arc::new(Config::load(Some(config_path)).context("loading configuration")?);
    init_tracing(&config)?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("constructing refresh runtime")?;

    let (index, backend_kind) = rt
        .block_on(connect_cache_backend(config.as_ref()))
        .context("connecting to cache index")?;
    let hot_cache_path = hot_cache_path(config.as_ref(), &backend_kind);
    let hot_cache =
        HotCache::open_with_config(&hot_cache_path, config.hotcache.reliability.clone())
            .context("opening hot cache")?;

    rt.block_on(hot_cache.refresh_from_index(index.as_ref()))
        .context("refreshing hot cache")?;

    drop(rt);

    let backend_label = match backend_kind {
        DatabaseBackend::Sqlite { path } => format!("SQLite ({})", path.display()),
        DatabaseBackend::Postgres { url, .. } => format!("PostgreSQL ({url})"),
    };

    println!(
        "Hot cache refreshed from {backend} (cache: {})",
        hot_cache_path.display(),
        backend = backend_label
    );

    Ok(())
}

fn run_catalog_sync(config_path: PathBuf) -> Result<()> {
    let config = Arc::new(Config::load(Some(config_path)).context("loading configuration")?);
    init_tracing(&config)?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("constructing catalog runtime")?;

    let (index, _backend_kind) = rt
        .block_on(connect_cache_backend(config.as_ref()))
        .context("connecting to cache index")?;

    match rt.block_on(catalog::sync_names_once(index.as_ref())) {
        Ok(Some(count)) => {
            println!("Catalogue synced: {} gem names processed", count);
        }
        Ok(None) => {
            println!("Catalogue already up to date");
        }
        Err(err) => {
            return Err(err.context("syncing catalogue"));
        }
    }

    Ok(())
}

fn run_quarantine_status(config_path: PathBuf) -> Result<()> {
    let config = Arc::new(Config::load(Some(config_path)).context("loading configuration")?);

    if !config.delay_policy.enabled {
        println!("Quarantine feature is disabled in configuration.");
        println!("Enable it by setting delay_policy.enabled = true in vein.toml");
        return Ok(());
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("constructing quarantine runtime")?;

    let (index, _) = rt
        .block_on(connect_cache_backend(config.as_ref()))
        .context("connecting to cache index")?;

    let stats = rt
        .block_on(index.quarantine_stats())
        .context("fetching quarantine stats")?;

    println!("Quarantine Status");
    println!("=================");
    println!("Quarantined:     {}", stats.total_quarantined);
    println!("Available:       {}", stats.total_available);
    println!("Pinned:          {}", stats.total_pinned);
    println!("Blocked/Yanked:  {}", stats.total_yanked);
    println!();
    println!("Releasing today:      {}", stats.versions_releasing_today);
    println!("Releasing this week:  {}", stats.versions_releasing_this_week);
    println!();
    println!("Default delay: {} days", config.delay_policy.default_delay_days);
    println!("Skip weekends: {}", config.delay_policy.skip_weekends);

    Ok(())
}

fn run_quarantine_list(config_path: PathBuf, limit: u32) -> Result<()> {
    let config = Arc::new(Config::load(Some(config_path)).context("loading configuration")?);

    if !config.delay_policy.enabled {
        println!("Quarantine feature is disabled in configuration.");
        return Ok(());
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("constructing quarantine runtime")?;

    let (index, _) = rt
        .block_on(connect_cache_backend(config.as_ref()))
        .context("connecting to cache index")?;

    let versions = rt
        .block_on(index.get_all_quarantined(limit, 0))
        .context("fetching quarantined versions")?;

    if versions.is_empty() {
        println!("No versions currently in quarantine.");
        return Ok(());
    }

    println!(
        "{:<30} {:<15} {:<15} {:<10} {}",
        "GEM", "VERSION", "PLATFORM", "STATUS", "AVAILABLE AFTER"
    );
    println!("{}", "-".repeat(90));

    for v in versions {
        let platform = v.platform.as_deref().unwrap_or("ruby");
        println!(
            "{:<30} {:<15} {:<15} {:<10} {}",
            v.name,
            v.version,
            platform,
            format!("{:?}", v.status),
            v.available_after.format("%Y-%m-%d %H:%M UTC")
        );
    }

    Ok(())
}

fn run_quarantine_promote(config_path: PathBuf) -> Result<()> {
    let config = Arc::new(Config::load(Some(config_path)).context("loading configuration")?);

    if !config.delay_policy.enabled {
        println!("Quarantine feature is disabled in configuration.");
        return Ok(());
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("constructing quarantine runtime")?;

    let (index, _) = rt
        .block_on(connect_cache_backend(config.as_ref()))
        .context("connecting to cache index")?;

    let count = rt
        .block_on(quarantine::promote_now(index.as_ref()))
        .context("promoting expired quarantines")?;

    if count > 0 {
        println!("Promoted {} version(s) from quarantine to available.", count);
    } else {
        println!("No versions ready for promotion.");
    }

    Ok(())
}

fn run_quarantine_approve(
    config_path: PathBuf,
    gem: String,
    version: String,
    platform: Option<String>,
    reason: String,
) -> Result<()> {
    use vein_adapter::VersionStatus;

    let config = Arc::new(Config::load(Some(config_path)).context("loading configuration")?);

    if !config.delay_policy.enabled {
        println!("Quarantine feature is disabled in configuration.");
        return Ok(());
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("constructing quarantine runtime")?;

    let (index, _) = rt
        .block_on(connect_cache_backend(config.as_ref()))
        .context("connecting to cache index")?;

    rt.block_on(index.update_version_status(
        &gem,
        &version,
        platform.as_deref(),
        VersionStatus::Pinned,
        Some(format!("approved: {}", reason)),
    ))
    .context("approving version")?;

    let platform_str = platform.as_deref().unwrap_or("ruby");
    println!(
        "Approved {}-{} ({}) for immediate availability.",
        gem, version, platform_str
    );
    println!("Reason: {}", reason);

    Ok(())
}

fn run_quarantine_block(
    config_path: PathBuf,
    gem: String,
    version: String,
    platform: Option<String>,
    reason: String,
) -> Result<()> {
    use vein_adapter::VersionStatus;

    let config = Arc::new(Config::load(Some(config_path)).context("loading configuration")?);

    if !config.delay_policy.enabled {
        println!("Quarantine feature is disabled in configuration.");
        return Ok(());
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("constructing quarantine runtime")?;

    let (index, _) = rt
        .block_on(connect_cache_backend(config.as_ref()))
        .context("connecting to cache index")?;

    rt.block_on(index.update_version_status(
        &gem,
        &version,
        platform.as_deref(),
        VersionStatus::Yanked,
        Some(format!("blocked: {}", reason)),
    ))
    .context("blocking version")?;

    let platform_str = platform.as_deref().unwrap_or("ruby");
    println!("Blocked {}-{} ({}).", gem, version, platform_str);
    println!("Reason: {}", reason);

    Ok(())
}
