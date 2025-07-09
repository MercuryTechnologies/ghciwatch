use std::path::PathBuf;

use camino::Utf8PathBuf;
use miette::miette;
use miette::Context;
use miette::IntoDiagnostic;

/// Get the current working directory of the process with [`std::env::current_dir`].
pub fn current_dir() -> miette::Result<PathBuf> {
    std::env::current_dir()
        .into_diagnostic()
        .wrap_err("Failed to get current directory")
}

/// Get the current working directory of the process as a [`Utf8PathBuf`].
pub fn current_dir_utf8() -> miette::Result<Utf8PathBuf> {
    current_dir()?
        .try_into()
        .map_err(|path| miette!("Current directory isn't valid UTF-8: {path:?}"))
}
