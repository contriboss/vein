use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use rama::telemetry::{
    opentelemetry::{
        KeyValue,
        collector::{SpanExporter, WithExportConfig},
        global,
        sdk::{resource::Resource, trace as sdktrace},
        trace::TracerProvider,
    },
    tracing::{
        self,
        subscriber::{
            self, EnvFilter, Layer as _, layer::SubscriberExt as _, util::SubscriberInitExt as _,
        },
    },
};
use tokio::runtime::{Builder, Runtime};
use vein::{
    config::{Config, DatabaseBackend},
    db::connect_cache_backend,
};
use vein_adapter::CacheBackend;

pub(crate) fn load_config(config_path: PathBuf) -> Result<Arc<Config>> {
    Ok(Arc::new(
        Config::load(Some(config_path)).context("loading configuration")?,
    ))
}

pub(crate) fn load_validated_config(config_path: PathBuf) -> Result<Arc<Config>> {
    let config = load_config(config_path)?;
    config.validate().context("validating configuration")?;
    Ok(config)
}

pub(crate) fn build_current_thread_runtime(context: &str) -> Result<Runtime> {
    Builder::new_current_thread()
        .enable_all()
        .build()
        .with_context(|| format!("constructing {context} runtime"))
}

pub(crate) fn connect_cache_index(
    rt: &Runtime,
    config: &Arc<Config>,
) -> Result<(Arc<CacheBackend>, DatabaseBackend)> {
    rt.block_on(connect_cache_backend(config.as_ref()))
        .context("connecting to cache index")
}

pub(crate) fn init_tracing(config: &Config) -> Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(&config.logging.level))
        .context("building log filter")?;

    let fmt_layer = if config.logging.json {
        subscriber::fmt::layer()
            .json()
            .flatten_event(true)
            .with_target(false)
            .boxed()
    } else {
        subscriber::fmt::layer().with_target(false).boxed()
    };

    let registry = subscriber::registry().with(filter).with(fmt_layer);

    if let Ok(endpoint) = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        let resource = Resource::builder_empty()
            .with_attributes([
                KeyValue::new("service.name", "vein"),
                KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
            ])
            .build();

        let exporter = SpanExporter::builder()
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
            .with(tracing::layer().with_tracer(tracer))
            .try_init()?;
    } else {
        registry.try_init()?;
    }

    Ok(())
}
