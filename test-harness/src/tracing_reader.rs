use std::path::Path;
use std::time::Duration;

use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use miette::Context;
use miette::IntoDiagnostic;
use tokio::fs::File;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::io::Lines;
use tokio::sync::mpsc;
use tracing::instrument;

use super::Event;

/// A task to read JSON tracing log events output by `ghid-ng` and send them over a channel.
#[allow(dead_code)]
pub struct TracingReader {
    sender: mpsc::Sender<Event>,
    lines: Lines<BufReader<File>>,
}

impl TracingReader {
    /// Create a new [`TracingReader`].
    ///
    /// This watches for data to be read from the given `path`. When a line is written to `path`
    /// (by `ghcid-ng`), the `TracingReader` will deserialize the line from JSON into an [`Event`]
    /// and send it to the given `sender` for another task to receive.
    pub async fn new(sender: mpsc::Sender<Event>, path: impl AsRef<Path>) -> miette::Result<Self> {
        let path = path.as_ref();

        let file = File::open(path)
            .await
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to open {path:?}"))?;

        let lines = BufReader::new(file).lines();

        Ok(Self { sender, lines })
    }

    /// Run this task.
    #[instrument(skip(self), name = "json-reader", level = "debug")]
    pub async fn run(mut self) -> miette::Result<()> {
        loop {
            match self.run_inner().await {
                Ok(()) => {
                    // Graceful shutdown
                    tracing::debug!("JSON reader exiting");
                    break;
                }
                Err(err) => {
                    tracing::error!("{err:?}");
                }
            }
        }

        Ok(())
    }

    async fn run_inner(&mut self) -> miette::Result<()> {
        let mut backoff = ExponentialBackoff {
            max_elapsed_time: None,
            max_interval: Duration::from_secs(1),
            ..Default::default()
        };

        while let Some(duration) = backoff.next_backoff() {
            while let Some(line) = self.lines.next_line().await.into_diagnostic()? {
                let event = serde_json::from_str(&line).into_diagnostic()?;
                self.sender.send(event).await.into_diagnostic()?;
            }
            tokio::time::sleep(duration).await;
        }

        Ok(())
    }
}
