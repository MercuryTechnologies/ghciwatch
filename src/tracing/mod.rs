//! Extensions and utilities for the [`tracing`] crate.
use std::io::Write;

use camino::Utf8Path;
use miette::Context;
use miette::IntoDiagnostic;
use opentelemetry::trace::{Tracer, TracerProvider as _};
use opentelemetry::InstrumentationScope;
use opentelemetry_otlp::ExportConfig;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tokio::io::DuplexStream;
use tokio_util::io::SyncIoBridge;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_opentelemetry::OpenTelemetryLayer;
// use tracing_perfetto::PerfettoLayer;
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::format::JsonFields;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
// use venator::Venator;

use crate::cli::Opts;

/// [`Drop`] guard for tracing stuff.
pub struct TracingGuard {
    /// TODO
    pub reader: Option<DuplexStream>,
    /// TODO
    pub worker_guard: WorkerGuard,
    /// TODO
    pub otel_tracer_provider: SdkTracerProvider,
}

/// Options for initializing the [`tracing`] logging framework. This is like a lower-effort builder
/// interface, mostly provided because Rust tragically lacks named arguments.
pub struct TracingOpts<'opts> {
    /// Filter directives to control which events are logged.
    pub filter_directives: &'opts str,
    /// Control which span events are logged.
    pub trace_spans: &'opts [FmtSpan],
    /// If given, log as JSON to the given path.
    pub json_log_path: Option<&'opts Utf8Path>,
    /// Are we running in TUI mode?
    ///
    /// A `(reader, writer)` pair to write to the TUI.
    pub tui: Option<(DuplexStream, DuplexStream)>, // Mutex<VecDeque>?
}

impl<'opts> TracingOpts<'opts> {
    /// Construct options for initializing the [`tracing`] logging framework from parsed
    /// commmand-line interface arguments as [`Opts`].
    pub fn from_cli(opts: &'opts Opts) -> Self {
        Self {
            filter_directives: &opts.logging.log_filter,
            trace_spans: &opts.logging.trace_spans,
            json_log_path: opts.logging.log_json.as_deref(),
            tui: if opts.tui {
                Some(tokio::io::duplex(crate::buffers::TRACING_BUFFER_CAPACITY))
            } else {
                None
            },
        }
    }

    /// Initialize the [`tracing`] logging framework.
    pub fn install(&mut self) -> miette::Result<TracingGuard> {
        let env_filter = EnvFilter::try_new(self.filter_directives).into_diagnostic()?;

        let fmt_span = self
            .trace_spans
            .iter()
            .fold(FmtSpan::NONE, |result, item| result | item.clone());

        let mut tracing_reader = None;
        let tracing_writer: Box<dyn Write + Send + Sync + 'static> = match self.tui.take() {
            Some((reader, writer)) => {
                tracing_reader = Some(reader);
                Box::new(SyncIoBridge::new(writer))
            }
            None => Box::new(std::io::stderr()),
        };
        let (tracing_writer, worker_guard) = tracing_appender::non_blocking(tracing_writer);

        let human_layer = tracing_human_layer::HumanLayer::default()
            .with_span_events(fmt_span.clone())
            .with_color_output(
                supports_color::on(supports_color::Stream::Stdout)
                    .map(|colors| colors.has_basic)
                    .unwrap_or(false),
            )
            .with_output_writer(tracing_writer)
            .with_filter(env_filter);

        // Initialize OTLP exporter using HTTP binary protocol
        let otlp_exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .build()
            .into_diagnostic()?;

        // Create a tracer provider with the exporter
        let tracer_provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
            .with_batch_exporter(otlp_exporter)
            .build();

        opentelemetry::global::set_tracer_provider(tracer_provider.clone());

        let scope = InstrumentationScope::builder(env!("CARGO_PKG_NAME"))
            .with_version(env!("CARGO_PKG_VERSION"))
            .with_schema_url("https://opentelemetry.io/schema/1.0.0")
            .build();

        let tracer = tracer_provider.tracer_with_scope(scope);

        init_tracing_opentelemetry::init_propagator().into_diagnostic()?;

        // Create a tracing layer with the configured tracer
        let otel_layer = OpenTelemetryLayer::new(tracer);

        let registry = tracing_subscriber::registry();

        let registry = registry.with(otel_layer).with(human_layer);

        match &self.json_log_path {
            Some(path) => {
                let json_layer = tracing_json_layer(self.filter_directives, path, fmt_span)?;
                registry.with(json_layer).init();
            }
            None => {
                registry.init();
            }
        }

        Ok(TracingGuard {
            reader: tracing_reader,
            worker_guard,
            otel_tracer_provider: tracer_provider,
        })
    }
}

fn tracing_json_layer<S>(
    filter_directives: &str,
    log_path: &Utf8Path,
    fmt_span: FmtSpan,
) -> miette::Result<Box<dyn tracing_subscriber::Layer<S> + Send + Sync + 'static>>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    let file = std::fs::File::create(log_path)
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to open {log_path:?}"))?;

    let env_filter = EnvFilter::try_new(filter_directives).into_diagnostic()?;

    let layer = fmt::layer()
        .with_span_events(fmt_span)
        .event_format(fmt::format::json())
        .fmt_fields(JsonFields::new())
        .with_writer(file)
        .with_filter(env_filter)
        .boxed();

    Ok(layer)
}
