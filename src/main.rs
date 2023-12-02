//! `ghciwatch` is a `ghci`-based file watcher and recompiler for Haskell projects, leveraging
//! Haskell's interpreted mode for faster reloads.
//!
//! `ghciwatch` watches your modules for changes and reloads them in a `ghci` session, displaying
//! any errors.

use std::time::Duration;

use async_dup::Arc;
use async_dup::Mutex;
use clap::Parser;
use ghciwatch::cli;
use ghciwatch::run_ghci;
use ghciwatch::run_watcher;
use ghciwatch::GhciOpts;
use ghciwatch::ShutdownManager;
use ghciwatch::TracingOpts;
use ghciwatch::WatcherOpts;
use ghciwatch::{run_tui, write_hello_world};
use tokio::sync::mpsc;
use tokio_util::compat::FuturesAsyncWriteCompatExt;
use tokio_util::compat::TokioAsyncWriteCompatExt;

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
    if opts.tui {
        let (tui_writer, tui_reader) = tokio::io::duplex(1024);
        let tui_writer = Arc::new(Mutex::new(tui_writer.compat_write())).compat_write();

        // ghci_opts.stdout_writer = tui_writer;

        manager
            .spawn("run_tui".to_owned(), |handle| {
                run_tui(handle, tui_reader, ghci_sender.clone())
            })
            .await;

        manager
            .spawn("write_hello_world".to_owned(), |_| {
                write_hello_world(tui_writer)
            })
            .await;
    }
    manager
        .spawn("run_ghci".to_owned(), |handle| {
            run_ghci(handle, ghci_opts, ghci_receiver)
        })
        .await;
    manager
        .spawn("run_watcher".to_owned(), |handle| {
            run_watcher(handle, ghci_sender.clone(), watcher_opts)
        })
        .await;
    let ret = manager.wait_for_shutdown().await;
    tracing::debug!("main() finished");
    ret
}
