use std::fmt::Display;
use std::path::Path;
use std::time::Duration;

use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use miette::Context;
use miette::IntoDiagnostic;
use tokio::fs::File;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

/// Touch a path.
pub async fn touch(path: impl AsRef<Path>) -> miette::Result<()> {
    let path = path.as_ref();
    OpenOptions::new()
        .create(true)
        .write(true)
        .open(path)
        .await
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to touch {path:?}"))
        .map(|_| ())
}

/// Append some data to a path.
pub async fn append(path: impl AsRef<Path>, data: impl Display) -> miette::Result<()> {
    let path = path.as_ref();
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .await
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to open {path:?}"))?;
    file.write_all(data.to_string().as_bytes())
        .await
        .into_diagnostic()?;
    Ok(())
}

/// Wait for a path to be created.
///
/// This should generally be run under a [`tokio::time::timeout`].
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
