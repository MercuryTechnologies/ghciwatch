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

mod format;

/// Initialize the logging framework.
pub fn install_tracing(
    filter_directives: &str,
    trace_spans: &[FmtSpan],
    json_log_path: Option<&Utf8Path>,
) -> miette::Result<()> {
    let env_filter = EnvFilter::try_new(filter_directives).into_diagnostic()?;

    let fmt_span = trace_spans
        .iter()
        .fold(FmtSpan::NONE, |result, item| result | item.clone());

    let fmt_layer = fmt::layer()
        .with_span_events(fmt_span.clone())
        .fmt_fields(format::SpanFieldFormatter::default())
        .event_format(format::EventFormatter::default())
        .with_filter(env_filter);

    let registry = tracing_subscriber::registry();

    let registry = registry.with(fmt_layer);

    match json_log_path {
        Some(path) => {
            let json_layer = tracing_json_layer(filter_directives, path, fmt_span)?;
            registry.with(json_layer).init();
        }
        None => {
            registry.init();
        }
    }

    Ok(())
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
