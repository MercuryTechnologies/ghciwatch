//! `ghciwatch` is a `ghci`-based file watcher and recompiler for Haskell projects, leveraging
//! Haskell's interpreted mode for faster reloads.
//!
//! `ghciwatch` watches your modules for changes and reloads them in a `ghci` session, displaying
//! any errors.

use std::time::Duration;

use clap::CommandFactory;
use clap::Parser;
use ghciwatch::cli;
use ghciwatch::run_ghci;
use ghciwatch::run_tui;
use ghciwatch::run_watcher;
use ghciwatch::GhciOpts;
use ghciwatch::ShutdownManager;
use ghciwatch::TracingOpts;
use ghciwatch::WatcherOpts;
use miette::IntoDiagnostic;
use opentelemetry::trace::SpanContext;
use opentelemetry::trace::TraceContextExt;
use tokio::io::DuplexStream;
use tokio::sync::mpsc;
use tracing::Instrument;

#[tokio::main]
async fn main() -> miette::Result<()> {
    miette::set_panic_hook();
    let mut opts = cli::Opts::parse();
    opts.init()?;
    let mut tracing_guard = TracingOpts::from_cli(&opts).install()?;

    #[cfg(feature = "clap-markdown")]
    if opts.generate_markdown_help {
        println!("{}", ghciwatch::clap_markdown::help_markdown::<cli::Opts>());
        return Ok(());
    }

    #[cfg(feature = "clap_mangen")]
    if let Some(out_dir) = opts.generate_man_pages {
        use miette::IntoDiagnostic;
        use miette::WrapErr;

        let command = cli::Opts::command();
        clap_mangen::generate_to(command, out_dir)
            .into_diagnostic()
            .wrap_err("Failed to generate man pages")?;
        return Ok(());
    }

    if let Some(shell) = opts.completions {
        let mut command = cli::Opts::command();
        clap_complete::generate(shell, &mut command, "ghciwatch", &mut std::io::stdout());
        return Ok(());
    }

    let ret = fuck(opts, tracing_guard.reader.take()).await;

    println!("end ctx: {:#?}", opentelemetry::Context::current());

    dbg!(tracing_guard
        .otel_tracer_provider
        .force_flush()
        .into_diagnostic()?);

    dbg!(tracing_guard
        .otel_tracer_provider
        .shutdown()
        .into_diagnostic()?);

    ret
}

#[tracing::instrument(skip_all)]
async fn fuck(opts: cli::Opts, tracing_reader: Option<DuplexStream>) -> miette::Result<()> {
    println!(
        "https://ui.honeycomb.io/mercury/environments/dev/datasets/ghciwatch/trace?trace_id={}",
        opentelemetry::Context::current()
            .span()
            .span_context()
            .trace_id()
    );
    std::env::set_var("IN_GHCIWATCH", "1");

    let (ghci_sender, ghci_receiver) = mpsc::channel(32);

    let (ghci_opts, maybe_ghci_reader) = GhciOpts::from_cli(&opts)?;
    let watcher_opts = WatcherOpts::from_cli(&opts);

    let mut manager = ShutdownManager::with_timeout(Duration::from_secs(1));

    if opts.tui {
        let tracing_reader =
            tracing_reader.expect("`tracing_reader` must be present if `tui` is given");
        let ghci_reader =
            maybe_ghci_reader.expect("`tui_reader` must be present if `tui` is given");
        manager
            .spawn("run_tui", |handle| {
                run_tui(handle, ghci_reader, tracing_reader)
            })
            .await;
    }

    manager
        .spawn("run_ghci", |handle| {
            run_ghci(handle, ghci_opts, ghci_receiver)
        })
        .await;
    manager
        .spawn("run_watcher", move |handle| {
            run_watcher(handle, ghci_sender, watcher_opts)
        })
        .await;
    let ret = manager.wait_for_shutdown().await;
    tracing::debug!("main() finished");
    ret
}
