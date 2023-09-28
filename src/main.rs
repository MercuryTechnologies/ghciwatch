//! `ghcid-ng` is a `ghci`-based file watcher and recompiler for Haskell projects, leveraging
//! Haskell's interpreted mode for faster reloads.
//!
//! `ghcid-ng` watches your modules for changes and reloads them in a `ghci` session, displaying
//! any errors.

use clap::Parser;
use ghcid_ng::cli;
use ghcid_ng::Ghci;
use ghcid_ng::GhciOpts;
use ghcid_ng::TracingOpts;
use ghcid_ng::Watcher;
use ghcid_ng::WatcherOpts;
use miette::IntoDiagnostic;
use miette::WrapErr;

#[tokio::main]
async fn main() -> miette::Result<()> {
    miette::set_panic_hook();
    let mut opts = cli::Opts::parse();
    opts.init()?;
    TracingOpts::from_cli(&opts).install()?;

    ::tracing::warn!(
        "This is a prerelease alpha version of `ghcid-ng`! Expect a rough user experience, and please report bugs or other issues to the #mighty-dux channel on Slack."
    );

    let ghci = Ghci::new(GhciOpts::from_cli(&opts)?)
        .await
        .wrap_err("Failed to start `ghci`")?;
    let watcher = Watcher::new(ghci, WatcherOpts::from_cli(&opts))
        .wrap_err("Failed to start file watcher")?;

    watcher
        .handle
        .await
        .into_diagnostic()?
        .wrap_err("File watcher failed")?;

    Ok(())
}
