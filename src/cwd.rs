use std::path::PathBuf;

use camino::Utf8PathBuf;
use eyre::eyre;
use eyre::Context;

/// Get the current working directory of the process with [`std::env::current_dir`].
pub fn current_dir() -> eyre::Result<PathBuf> {
    std::env::current_dir().wrap_err("Failed to get current directory")
}

/// Get the current working directory of the process as a [`Utf8PathBuf`].
pub fn current_dir_utf8() -> eyre::Result<Utf8PathBuf> {
    current_dir()?
        .try_into()
        .map_err(|path| eyre!("Current directory isn't valid UTF-8: {path:?}"))
}
