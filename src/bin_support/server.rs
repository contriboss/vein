use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use rama::{
    Layer as RamaLayer, Service,
    graceful::Shutdown,
    http::{
        StatusCode,
        layer::{
            error_handling::ErrorHandlerLayer,
            request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
            trace::TraceLayer,
        },
        server::HttpServer,
    },
    rt::Executor,
    tcp::{TcpStream, server::TcpListener},
    telemetry::tracing,
    tls::rustls::dep::rustls,
};
use vein::{proxy::VeinProxy, quarantine};
use vein_adapter::FilesystemStorage;

use super::setup::{
    build_current_thread_runtime, connect_cache_index, init_tracing, load_validated_config,
};

pub fn run_server(config_path: PathBuf) -> Result<()> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let config = load_validated_config(config_path)?;

    config
        .storage
        .ensure_directories()
        .context("creating storage directories")?;

    init_tracing(&config)?;

    let setup_rt = build_current_thread_runtime("setup")?;

    let storage = Arc::new(FilesystemStorage::new(config.storage.path.clone()));
    setup_rt
        .block_on(storage.prepare())
        .context("preparing storage directory")?;

    let (index, _) = connect_cache_index(&setup_rt, &config)?;

    quarantine::spawn_promotion_scheduler(&config.delay_policy, index.clone(), None);

    drop(setup_rt);

    let proxy = VeinProxy::new(config.clone(), storage.clone(), index.clone())
        .context("creating proxy service")?;

    let server_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(config.server.workers)
        .enable_all()
        .build()
        .context("constructing server runtime")?;

    server_rt.block_on(async move {
        let graceful = Shutdown::new(shutdown_signal());
        let addr = format!("{}:{}", config.server.host, config.server.port);

        tracing::info!(%addr, "starting Rama HTTP server");

        spawn_listener(&graceful, addr, "proxy", move |exec| {
            HttpServer::auto(exec).service(
                (
                    SetRequestIdLayer::x_request_id(MakeRequestUuid),
                    PropagateRequestIdLayer::x_request_id(),
                    TraceLayer::new_for_http(),
                    // VeinProxy logs the failure; the client gets a 500
                    // rather than an empty 200.
                    ErrorHandlerLayer::new().error_mapper(|_| StatusCode::INTERNAL_SERVER_ERROR),
                )
                    .into_layer(proxy),
            )
        });

        spawn_admin(&graceful, config, index, storage).await?;

        // Resolves once `shutdown_signal` fires and in-flight work drains.
        graceful
            .shutdown_with_limit(Duration::from_secs(30))
            .await?;

        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}

/// Serve the admin dashboard on its own listener when `[admin]` is enabled.
#[cfg(feature = "admin")]
async fn spawn_admin(
    graceful: &Shutdown,
    config: Arc<vein::config::Config>,
    index: Arc<vein_adapter::CacheBackend>,
    storage: Arc<FilesystemStorage>,
) -> Result<()> {
    if !config.admin.enabled {
        return Ok(());
    }
    let addr = config.admin.bind_addr();
    let router = vein::admin::router(config, index, storage)
        .await
        .context("building admin dashboard")?;
    tracing::info!(%addr, "starting admin server");

    spawn_listener(graceful, addr, "admin", move |exec| {
        // Router misses become 404/405 instead of an empty 200.
        HttpServer::auto(exec).service(Arc::new(ErrorHandlerLayer::new().into_layer(router)))
    });
    Ok(())
}

/// Bind `addr` and serve `make_service(exec)` on it until shutdown; the task
/// panics if the bind fails, which takes the whole server down at startup.
fn spawn_listener<S, F>(graceful: &Shutdown, addr: String, what: &'static str, make_service: F)
where
    F: FnOnce(Executor) -> S + Send + 'static,
    S: Service<TcpStream> + Clone,
{
    graceful.spawn_task_fn(move |guard| async move {
        let tcp = TcpListener::build(Executor::graceful(guard.clone()))
            .bind_address(addr)
            .await
            .unwrap_or_else(|err| panic!("bind {what} listener: {err}"));
        tcp.serve(make_service(Executor::graceful(guard))).await;
    });
}

/// Builds without the `admin` feature only report that the section is ignored.
#[cfg(not(feature = "admin"))]
#[expect(clippy::unused_async, reason = "same call site as the admin build")]
async fn spawn_admin(
    _graceful: &Shutdown,
    config: Arc<vein::config::Config>,
    _index: Arc<vein_adapter::CacheBackend>,
    _storage: Arc<FilesystemStorage>,
) -> Result<()> {
    if config.admin.enabled {
        tracing::warn!("[admin] is enabled but this build has no `admin` feature");
    }
    Ok(())
}

/// Ctrl-C or SIGTERM, so `docker stop` and systemd get a graceful exit.
async fn shutdown_signal() {
    rama::graceful::default_signal().await;
}
