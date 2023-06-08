use clap::Parser;
use miette::IntoDiagnostic;
use miette::WrapErr;

mod aho_corasick;
mod buffers;
mod clap_camino;
mod cli;
mod command;
mod event_filter;
mod ghci;
mod haskell_show;
mod incremental_reader;
mod lines;
mod sync_sentinel;
mod textwrap;
mod tracing;
mod watcher;

#[cfg(test)]
mod fake_reader;

use ghci::Ghci;
use watcher::Watcher;

#[tokio::main]
async fn main() -> miette::Result<()> {
    cli::Opts::parse().set_opts();
    cli::with_opts(|opts| tracing::install_tracing(&opts.logging.tracing_filter))?;

    ::tracing::warn!(
        "This is a prerelease alpha version of `ghcid-ng`! Expect a rough user experience, and please report bugs or other issues to the #mighty-dux channel on Slack."
    );

    // TODO: implement fancier default command
    // See: https://github.com/ndmitchell/ghcid/blob/e2852979aa644c8fed92d46ab529d2c6c1c62b59/src/Ghcid.hs#L142-L171
    let ghci_command = || {
        cli::with_opts(|opts| command::from_string(opts.command.as_deref().unwrap_or("ghci")))
            .wrap_err("Failed to split `--command` value into arguments")
    };

    let ghci = Ghci::new(ghci_command).await?;
    let watcher = cli::with_opts(|opts| Watcher::new(ghci, &opts.watch))?;

    watcher.handle.await.into_diagnostic()??;

    Ok(())
}
