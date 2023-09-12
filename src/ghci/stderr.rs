use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use tokio::io::BufReader;
use tokio::io::Lines;
use tokio::process::ChildStderr;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::instrument;

use super::Mode;

/// An event sent to a `ghci` session's stderr channel.
#[derive(Debug)]
pub enum StderrEvent {
    /// Set the writer's mode.
    Mode {
        mode: Mode,
        sender: oneshot::Sender<()>,
    },

    /// Get the buffer contents since the last `Mode` event.
    GetBuffer { sender: oneshot::Sender<String> },
}

pub struct GhciStderr {
    pub reader: Lines<BufReader<ChildStderr>>,
    pub receiver: mpsc::Receiver<StderrEvent>,
    /// Output buffer.
    pub buffer: String,
    /// The mode we're currently reading output in.
    pub mode: Mode,
}

impl GhciStderr {
    #[instrument(skip_all, name = "stderr", level = "debug")]
    pub async fn run(mut self) -> miette::Result<()> {
        let mut backoff = ExponentialBackoff::default();
        while let Some(duration) = backoff.next_backoff() {
            match self.run_inner().await {
                Ok(()) => {
                    // MPSC channel closed, probably a graceful shutdown?
                    tracing::debug!("Channel closed");
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
            }
        }
    }

    async fn dispatch(&mut self, event: StderrEvent) -> miette::Result<()> {
        match event {
            StderrEvent::Mode { mode, sender } => {
                self.set_mode(sender, mode).await;
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

    #[instrument(skip(self, sender), level = "trace")]
    async fn set_mode(&mut self, sender: oneshot::Sender<()>, mode: Mode) {
        self.mode = mode;
        self.buffer.clear();
        let _ = sender.send(());
    }

    #[instrument(skip(self, sender), level = "debug")]
    async fn get_buffer(&mut self, sender: oneshot::Sender<String>) {
        // TODO: Does it make more sense to clear the buffer here?
        let _ = sender.send(self.buffer.clone());
    }
}
