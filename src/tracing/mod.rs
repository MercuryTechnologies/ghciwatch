//! Extensions and utilities for the [`tracing`] crate.

use miette::IntoDiagnostic;
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;

mod format;

/// Initialize the logging framework.
pub fn install_tracing(filter_directives: &str) -> miette::Result<()> {
    let env_filter = EnvFilter::try_new(filter_directives)
        .or_else(|_| EnvFilter::try_from_default_env())
        .or_else(|_| EnvFilter::try_new("info"))
        .into_diagnostic()?;

    let fmt_layer = fmt::layer()
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
        .fmt_fields(format::SpanFieldFormatter::default())
        .event_format(format::EventFormatter::default())
        .with_filter(env_filter);

    let registry = tracing_subscriber::registry();

    let registry = registry.with(fmt_layer);

    registry.init();
    Ok(())
}
