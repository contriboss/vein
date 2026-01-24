use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use rama::{
    Layer as RamaLayer,
    graceful::Shutdown,
    http::{
        layer::{
            request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
            trace::TraceLayer,
        },
        server::HttpServer,
    },
    layer::ConsumeErrLayer,
    rt::Executor,
    tcp::server::TcpListener,
    telemetry::tracing,
    tls::rustls::dep::rustls,
};
use vein::{proxy::VeinProxy, quarantine};
use vein_adapter::FilesystemStorage;

use super::setup::{
    build_current_thread_runtime, connect_cache_index, init_tracing, load_validated_config,
};

pub(crate) fn run_server(config_path: PathBuf) -> Result<()> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let config = load_validated_config(config_path)?;

    config
        .storage
        .ensure_directories()
        .context("creating storage directories")?;
    config
        .database
        .ensure_directories()
        .context("creating database directories")?;

    init_tracing(&config)?;

    let setup_rt = build_current_thread_runtime("setup")?;

    let storage = Arc::new(FilesystemStorage::new(config.storage.path.clone()));
    setup_rt
        .block_on(storage.prepare())
        .context("preparing storage directory")?;

    let (index, _) = connect_cache_index(&setup_rt, &config)?;

    quarantine::spawn_promotion_scheduler(&config.delay_policy, index.clone(), None);

    drop(setup_rt);

    let proxy = VeinProxy::new(config.clone(), storage, index).context("creating proxy service")?;

    let server_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(config.server.workers)
        .enable_all()
        .build()
        .context("constructing server runtime")?;

    server_rt.block_on(async move {
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
                    (
                        SetRequestIdLayer::x_request_id(MakeRequestUuid),
                        PropagateRequestIdLayer::x_request_id(),
                        TraceLayer::new_for_http(),
                        ConsumeErrLayer::default(),
                    )
                        .into_layer(proxy),
                );

                tcp_service.serve_graceful(guard, http_service).await;
            }
        });

        wait_for_shutdown().await;

        graceful
            .shutdown_with_limit(Duration::from_secs(30))
            .await?;

        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}

#[cfg(not(target_os = "android"))]
async fn wait_for_shutdown() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for shutdown signal");
}

#[cfg(target_os = "android")]
async fn wait_for_shutdown() {
    std::future::pending::<()>().await
}
