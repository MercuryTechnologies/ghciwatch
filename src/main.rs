//! `ghciwatch` is a `ghci`-based file watcher and recompiler for Haskell projects, leveraging
//! Haskell's interpreted mode for faster reloads.
//!
//! `ghciwatch` watches your modules for changes and reloads them in a `ghci` session, displaying
//! any errors.

use std::time::Duration;

use clap::Parser;
use ghciwatch::cli;
use ghciwatch::run_ghci;
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
    TracingOpts::from_cli(&opts).install()?;

    let (ghci_sender, ghci_receiver) = mpsc::channel(32);

    let ghci_opts = GhciOpts::from_cli(&opts)?;
    let watcher_opts = WatcherOpts::from_cli(&opts);

    let mut manager = ShutdownManager::with_timeout(Duration::from_secs(1));
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
