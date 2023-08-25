//! `ghcid-ng` is a `ghci`-based file watcher and recompiler for Haskell projects, leveraging
//! Haskell's interpreted mode for faster reloads.
//!
//! `ghcid-ng` watches your modules for changes and reloads them in a `ghci` session, displaying
//! any errors.

use std::sync::Arc;

use clap::Parser;
use ghcid_ng::cli;
use ghcid_ng::command;
use ghcid_ng::ghci::Ghci;
use ghcid_ng::tracing;
use ghcid_ng::watcher::Watcher;
use miette::IntoDiagnostic;
use miette::WrapErr;
use tap::Tap;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> miette::Result<()> {
    miette::set_panic_hook();
    let opts = cli::Opts::parse().tap_mut(|opts| opts.init());
    tracing::install_tracing(
        &opts.logging.tracing_filter,
        &opts.logging.trace_spans,
        opts.logging.log_json.as_deref(),
    )?;

    ::tracing::warn!(
        "This is a prerelease alpha version of `ghcid-ng`! Expect a rough user experience, and please report bugs or other issues to the #mighty-dux channel on Slack."
    );

    // TODO: implement fancier default command
    // See: https://github.com/ndmitchell/ghcid/blob/e2852979aa644c8fed92d46ab529d2c6c1c62b59/src/Ghcid.hs#L142-L171
    let ghci_command = Arc::new(Mutex::new(
        command::from_string(opts.command.as_deref().unwrap_or("cabal repl"))
            .wrap_err("Failed to split `--command` value into arguments")?,
    ));

    let ghci = Ghci::new(ghci_command, opts.errors.clone(), opts.setup, opts.test)
        .await
        .wrap_err("Failed to start `ghci`")?;
    let watcher = Watcher::new(
        ghci,
        &opts.watch.paths,
        opts.watch.debounce,
        opts.watch.poll,
    )
    .wrap_err("Failed to start file watcher")?;

    watcher
        .handle
        .await
        .into_diagnostic()?
        .wrap_err("File watcher failed")?;

    Ok(())
}
