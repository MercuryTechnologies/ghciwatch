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

use super::Event;

/// A task to read JSON tracing log events output by `ghid-ng` and send them over a channel.
pub struct TracingReader {
    lines: Lines<BufReader<File>>,
}

impl TracingReader {
    /// Create a new [`TracingReader`].
    ///
    /// This watches for data to be read from the given `path`. When a line is written to `path`
    /// (by `ghcid-ng`), the `TracingReader` will deserialize the line from JSON into an [`Event`]
    /// and send it to the given `sender` for another task to receive.
    pub async fn new(path: impl AsRef<Path>) -> miette::Result<Self> {
        let path = path.as_ref();

        let file = File::open(path)
            .await
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to open {path:?}"))?;

        let lines = BufReader::new(file).lines();

        Ok(Self { lines })
    }

    /// Read the next event from the contained file.
    ///
    /// This will block indefinitely until a line is written to the contained file.
    pub async fn next_event(&mut self) -> miette::Result<Event> {
        let mut backoff = ExponentialBackoff {
            max_elapsed_time: None,
            max_interval: Duration::from_secs(1),
            ..Default::default()
        };

        while let Some(duration) = backoff.next_backoff() {
            if let Some(line) = self.lines.next_line().await.into_diagnostic()? {
                let event = serde_json::from_str(&line)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("Failed to deserialize JSON: {line}"))?;
                return Ok(event);
            }
            tokio::time::sleep(duration).await;
        }

        unreachable!()
    }
}
