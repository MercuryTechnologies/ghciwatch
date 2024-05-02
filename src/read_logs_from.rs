use std::io::SeekFrom;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::time::Duration;

use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use camino::Utf8Path;
use camino::Utf8PathBuf;
use fs_err::tokio as fs;
use fs_err::tokio::File;
use miette::miette;
use miette::IntoDiagnostic;
use tap::TryConv;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncSeekExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tracing::instrument;

use crate::ShutdownHandle;

/// Maximum number of bytes to print near the end of a log file, if it already has data when it's
/// opened.
const MAX_BYTES_PRINT_FROM_END: u64 = 0x200; // = 512

/// Me: Can we have `tail(1)`?
///
/// `ghciwatch`: We have `tail(1)` at home.
///
/// `tail(1)` at home:
pub struct ReadLogsFrom {
    /// Shutdown handle.
    pub shutdown: ShutdownHandle,
    /// Path to read logs from.
    pub path: Utf8PathBuf,
}

impl ReadLogsFrom {
    /// Read logs from the given path and output them to stdout.
    #[instrument(skip_all, name = "read-logs", level = "debug", fields(path = %self.path))]
    pub async fn run(mut self) -> miette::Result<()> {
        let mut backoff = ExponentialBackoff {
            max_elapsed_time: None,
            max_interval: Duration::from_secs(1),
            ..Default::default()
        };
        while let Some(duration) = backoff.next_backoff() {
            match self.run_inner().await {
                Ok(()) => {
                    // Graceful exit.
                    break;
                }
                Err(err) => {
                    // These errors are often like "the file doesn't exist yet" so we don't want
                    // them to be noisy.
                    tracing::debug!("{err:?}");
                }
            }

            tracing::debug!("Waiting {duration:?} before retrying");
            tokio::time::sleep(duration).await;
        }

        Ok(())
    }

    async fn run_inner(&mut self) -> miette::Result<()> {
        loop {
            tokio::select! {
                result = Self::read(&self.path) => {
                    result?;
                }
                _ = self.shutdown.on_shutdown_requested() => {
                    // Graceful exit.
                    break;
                }
                else => {
                    // Graceful exit.
                    break;
                }
            }
        }
        Ok(())
    }

    async fn read(path: &Utf8Path) -> miette::Result<()> {
        let file = File::open(&path).await.into_diagnostic()?;
        let mut metadata = file.metadata().await.into_diagnostic()?;
        let mut size = metadata.len();
        let mut reader = BufReader::new(file);

        if size > MAX_BYTES_PRINT_FROM_END {
            tracing::debug!("Log file too big, skipping to end");
            reader
                .seek(SeekFrom::End(
                    -MAX_BYTES_PRINT_FROM_END
                        .try_conv::<i64>()
                        .expect("Constant is not bigger than i64::MAX"),
                ))
                .await
                .into_diagnostic()?;
        }

        let mut lines = reader.lines();

        let mut backoff = ExponentialBackoff {
            max_elapsed_time: None,
            max_interval: Duration::from_millis(1000),
            ..Default::default()
        };

        let mut stdout = tokio::io::stdout();

        while let Some(duration) = backoff.next_backoff() {
            while let Some(line) = lines.next_line().await.into_diagnostic()? {
                // TODO: Lock stdout here and for ghci output.
                let _ = stdout.write_all(line.as_bytes()).await;
                let _ = stdout.write_all(b"\n").await;
            }

            // Note: This will fail if the file has been removed. The inode/device number check is
            // a secondary heuristic.
            let new_metadata = fs::metadata(&path).await.into_diagnostic()?;
            #[cfg(unix)]
            if new_metadata.dev() != metadata.dev() || new_metadata.ino() != metadata.ino() {
                return Err(miette!("Log file was replaced or removed: {path}"));
            }

            let new_size = new_metadata.len();
            if new_size < size {
                tracing::info!(%path, "Log file truncated");
                let mut reader = lines.into_inner();
                reader.seek(SeekFrom::Start(0)).await.into_diagnostic()?;
                lines = reader.lines();
            }
            size = new_size;
            metadata = new_metadata;

            tracing::trace!("Caught up to log file");

            tracing::trace!("Waiting {duration:?} before retrying");
            tokio::time::sleep(duration).await;
        }

        Ok(())
    }
}
