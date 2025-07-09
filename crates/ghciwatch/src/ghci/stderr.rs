use std::time::Duration;
use std::time::Instant;

use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use miette::Context;
use miette::IntoDiagnostic;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::io::Lines;
use tokio::process::ChildStderr;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::instrument;

use crate::shutdown::ShutdownHandle;

use super::writer::GhciWriter;

/// An event sent to a `ghci` session's stderr channel.
#[derive(Debug)]
pub enum StderrEvent {
    /// Clear the buffer contents.
    ClearBuffer,

    /// Get the buffer contents since the last `ClearBuffer` event.
    GetBuffer { sender: oneshot::Sender<String> },
}

pub struct GhciStderr {
    pub shutdown: ShutdownHandle,
    pub reader: Lines<BufReader<ChildStderr>>,
    pub writer: GhciWriter,
    pub receiver: mpsc::Receiver<StderrEvent>,
    /// Output buffer.
    pub buffer: String,
}

impl GhciStderr {
    #[instrument(skip_all, name = "stderr", level = "debug")]
    pub async fn run(mut self) -> miette::Result<()> {
        let mut backoff = ExponentialBackoff::default();
        while let Some(duration) = backoff.next_backoff() {
            match self.run_inner().await {
                Ok(()) => {
                    // MPSC channel closed, probably a graceful shutdown?
                    break;
                }
                Err(err) => {
                    tracing::error!("{err:?}");
                }
            }

            tracing::debug!("Waiting {duration:?} before retrying");
            tokio::time::sleep(duration).await;
        }

        Ok(())
    }

    pub async fn run_inner(&mut self) -> miette::Result<()> {
        loop {
            tokio::select! {
                Ok(Some(line)) = self.reader.next_line() => {
                    self.ingest_line(line).await?;
                }
                Some(event) = self.receiver.recv() => {
                    self.dispatch(event).await?;
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

    async fn dispatch(&mut self, event: StderrEvent) -> miette::Result<()> {
        match event {
            StderrEvent::ClearBuffer => {
                self.clear_buffer().await;
            }
            StderrEvent::GetBuffer { sender } => {
                self.get_buffer(sender).await?;
            }
        }

        Ok(())
    }

    #[instrument(skip(self), level = "trace")]
    async fn ingest_line(&mut self, mut line: String) -> miette::Result<()> {
        tracing::debug!(line, "Read stderr line");
        line.push('\n');
        self.buffer.push_str(&line);
        self.writer
            .write_all(line.as_bytes())
            .await
            .into_diagnostic()?;
        Ok(())
    }

    #[instrument(skip(self), level = "trace")]
    async fn clear_buffer(&mut self) {
        self.buffer.clear();
    }

    #[instrument(skip(self, sender), level = "debug")]
    async fn get_buffer(&mut self, sender: oneshot::Sender<String>) -> miette::Result<()> {
        // Read lines from the stderr stream until we can't read a line within 0.05 seconds.
        //
        // This helps make sure we've read all the available data.
        //
        // In testing, this takes ~52ms.
        let start_instant = Instant::now();
        while let Ok(maybe_line) =
            tokio::time::timeout(Duration::from_millis(50), self.reader.next_line()).await
        {
            match maybe_line
                .into_diagnostic()
                .wrap_err("Failed to read stderr line")?
            {
                Some(line) => {
                    self.ingest_line(line).await?;
                }
                None => {
                    tracing::debug!("No more lines available from stderr");
                }
            }
        }
        tracing::debug!("Drained stderr buffer in {:.2?}", start_instant.elapsed());

        // TODO: Does it make more sense to clear the buffer here?
        let _ = sender.send(self.buffer.clone());

        Ok(())
    }
}
