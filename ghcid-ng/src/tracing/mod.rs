//! Extensions and utilities for the [`tracing`] crate.

use camino::Utf8Path;
use miette::Context;
use miette::IntoDiagnostic;
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::format::JsonFields;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;

use crate::cli::Opts;

mod format;

/// Options for initializing the [`tracing`] logging framework. This is like a lower-effort builder
/// interface, mostly provided because Rust tragically lacks named arguments.
pub struct TracingOpts<'opts> {
    /// Filter directives to control which events are logged.
    pub filter_directives: &'opts str,
    /// Control which span events are logged.
    pub trace_spans: &'opts [FmtSpan],
    /// If given, log as JSON to the given path.
    pub json_log_path: Option<&'opts Utf8Path>,
}

impl<'opts> TracingOpts<'opts> {
    /// Construct options for initializing the [`tracing`] logging framework from parsed
    /// commmand-line interface arguments as [`Opts`].
    pub fn from_cli(opts: &'opts Opts) -> Self {
        Self {
            filter_directives: &opts.logging.tracing_filter,
            trace_spans: &opts.logging.trace_spans,
            json_log_path: opts.logging.log_json.as_deref(),
        }
    }

    /// Initialize the [`tracing`] logging framework.
    pub fn install(&self) -> miette::Result<()> {
        let env_filter = EnvFilter::try_new(self.filter_directives).into_diagnostic()?;

        let fmt_span = self
            .trace_spans
            .iter()
            .fold(FmtSpan::NONE, |result, item| result | item.clone());

        let fmt_layer = fmt::layer()
            .with_span_events(fmt_span.clone())
            .fmt_fields(format::SpanFieldFormatter::default())
            .event_format(format::EventFormatter::default())
            .with_filter(env_filter);

        let registry = tracing_subscriber::registry();

        let registry = registry.with(fmt_layer);

        match &self.json_log_path {
            Some(path) => {
                let json_layer = tracing_json_layer(self.filter_directives, path, fmt_span)?;
                registry.with(json_layer).init();
            }
            None => {
                registry.init();
            }
        }

        Ok(())
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
