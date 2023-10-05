//! Filesystem utilities for writing integration tests for `ghciwatch`.

use std::fmt::Debug;
use std::path::Path;
use std::time::Duration;

use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use miette::miette;
use miette::Context;
use miette::IntoDiagnostic;
use tokio::fs::File;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

/// Touch a path.
#[tracing::instrument]
pub async fn touch(path: impl AsRef<Path> + Debug) -> miette::Result<()> {
    let path = path.as_ref();
    if path.exists() {
        // I've had trouble with the pure-`open` approach getting detected, so let's actually
        // write the file's contents again.
        let contents = read(path).await?;
        write(path, contents).await
    } else {
        if let Some(parent) = path.parent() {
            create_dir(parent).await?;
        }
        OpenOptions::new()
            .create(true)
            .write(true)
            .open(path)
            .await
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to touch {path:?}"))
            .map(|_| ())
    }
}

/// Write some data to a path, replacing its previous contents.
#[tracing::instrument(skip(data))]
pub async fn write(path: impl AsRef<Path> + Debug, data: impl AsRef<[u8]>) -> miette::Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        create_dir(parent).await?;
    }

    // Load-bearing sleep! If this is removed, some writes aren't detected some of the time.
    // Comment it out and run `cargo nextest run` in a loop to see what I mean.
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    tokio::fs::write(path, data)
        .await
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to write {path:?}"))
}

/// Append some data to a path.
#[tracing::instrument(skip(data))]
pub async fn append(path: impl AsRef<Path> + Debug, data: impl AsRef<[u8]>) -> miette::Result<()> {
    let path = path.as_ref();
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .await
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to open {path:?}"))?;
    file.write_all(data.as_ref()).await.into_diagnostic()
}

/// Wait for a path to be created.
///
/// This should generally be run under a [`tokio::time::timeout`].
#[tracing::instrument]
pub async fn wait_for_path(path: &Path) {
    let mut backoff = ExponentialBackoff {
        max_interval: Duration::from_secs(1),
        ..Default::default()
    };
    while let Some(duration) = backoff.next_backoff() {
        if (File::open(path).await).is_ok() {
            break;
        }
        tracing::debug!("Waiting {duration:?} before retrying");
        tokio::time::sleep(duration).await;
    }
}

/// Read a path into a string.
#[tracing::instrument]
pub async fn read(path: impl AsRef<Path> + Debug) -> miette::Result<String> {
    let path = path.as_ref();
    tokio::fs::read_to_string(path)
        .await
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to read {path:?}"))
}

/// Read from a path, run a string replacement on its contents, and then write the result.
#[tracing::instrument(skip(from, to))]
pub async fn replace(
    path: impl AsRef<Path> + Debug,
    from: impl AsRef<str>,
    to: impl AsRef<str>,
) -> miette::Result<()> {
    let path = path.as_ref();
    let old_contents = read(path).await?;
    let new_contents = old_contents.replace(from.as_ref(), to.as_ref());
    if old_contents == new_contents {
        return Err(miette!(
            "Replacing substring in file didn't make any changes"
        ));
    }
    write(path, new_contents).await
}

/// Creates a directory and all of its parent components.
#[tracing::instrument]
pub async fn create_dir(path: impl AsRef<Path> + Debug) -> miette::Result<()> {
    let path = path.as_ref();
    tokio::fs::create_dir_all(path)
        .await
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to create directory {path:?}"))
}

/// Remove the file or directory at the given path.
///
/// Directories are removed recursively; be careful.
#[tracing::instrument]
pub async fn remove(path: impl AsRef<Path> + Debug) -> miette::Result<()> {
    let path = path.as_ref();
    if path.is_dir() {
        tokio::fs::remove_dir_all(path).await
    } else {
        tokio::fs::remove_file(path).await
    }
    .into_diagnostic()
    .wrap_err_with(|| format!("Failed to remove {path:?}"))
}

/// Move the path at `from` to the path at `to`.
#[tracing::instrument]
pub async fn rename(
    from: impl AsRef<Path> + Debug,
    to: impl AsRef<Path> + Debug,
) -> miette::Result<()> {
    let from = from.as_ref();
    let to = to.as_ref();
    tokio::fs::rename(from, to)
        .await
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to move {from:?} to {to:?}"))
}
