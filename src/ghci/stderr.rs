use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use tokio::io::BufReader;
use tokio::io::Lines;
use tokio::process::ChildStderr;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::instrument;

use crate::shutdown::ShutdownHandle;

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
            // TODO: Could this cause problems where we get an event and a final stderr line is only
            // processed after we write the error log?
            tokio::select! {
                Ok(Some(line)) = self.reader.next_line() => {
                    self.ingest_line(line).await;
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
                self.get_buffer(sender).await;
            }
        }

        Ok(())
    }

    #[instrument(skip(self), level = "trace")]
    async fn ingest_line(&mut self, line: String) {
        // Then write to our general buffer.
        self.buffer.push_str(&line);
        self.buffer.push('\n');
        eprintln!("{line}");
    }

    #[instrument(skip(self), level = "trace")]
    async fn clear_buffer(&mut self) {
        self.buffer.clear();
    }

    #[instrument(skip(self, sender), level = "debug")]
    async fn get_buffer(&mut self, sender: oneshot::Sender<String>) {
        // TODO: Does it make more sense to clear the buffer here?
        let _ = sender.send(self.buffer.clone());
    }
}
