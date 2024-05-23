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
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> miette::Result<()> {
    miette::set_panic_hook();
    let mut opts = cli::Opts::parse();
    opts.init()?;
    let (maybe_tracing_reader, _tracing_guard) = TracingOpts::from_cli(&opts).install()?;

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

    std::env::set_var("IN_GHCIWATCH", "1");

    let (ghci_sender, ghci_receiver) = mpsc::channel(32);

    let (ghci_opts, maybe_ghci_reader) = GhciOpts::from_cli(&opts)?;
    let watcher_opts = WatcherOpts::from_cli(&opts);

    let mut manager = ShutdownManager::with_timeout(Duration::from_secs(1));

    if opts.tui {
        let tracing_reader =
            maybe_tracing_reader.expect("`tracing_reader` must be present if `tui` is given");
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
