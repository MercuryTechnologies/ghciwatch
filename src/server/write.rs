use miette::IntoDiagnostic;
use serde::Serialize;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;
use tokio::sync::broadcast;
use tracing::instrument;

/// A `ghcid-ng` server notification. `ghcid-ng` writes these to a port or socket to inform clients
/// of actions and events from `ghcid-ng`.
#[derive(Debug, Clone, Serialize)]
pub enum ServerNotification {
    /// The server is reloading the `ghci` session.
    Reload,
    /// The server is exiting.
    Exit,
}

/// Task for writing notifications and events to a port or socket (in the form of
/// [`ServerNotification`]s). This is generic over the underlying writer so it will work with
/// Unix Domain Sockets, TCP ports, etc.
pub struct ServerWrite<W> {
    /// The underlying writer.
    writer: W,
    /// A channel for communicating with this task.
    receiver: broadcast::Receiver<ServerNotification>,
}

impl<W> ServerWrite<W>
where
    W: AsyncWrite + Unpin,
{
    /// Create a new task to write notifications to the given `writer`.
    pub fn new(writer: W, receiver: broadcast::Receiver<ServerNotification>) -> Self {
        Self { writer, receiver }
    }

    /// Run this task, writing events to the wrapped port as they're received.
    #[instrument(skip_all, name = "server-read", level = "debug")]
    pub async fn run(mut self) -> miette::Result<()> {
        loop {
            match self.run_inner().await {
                Ok(()) => {
                    // Channel close; graceful shutdown.
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
        loop {
            match self.receiver.recv().await {
                Ok(notification) => {
                    self.write(notification).await?;
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }

        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    async fn write(&mut self, notification: ServerNotification) -> miette::Result<()> {
        // TODO: There is a `serde_json::to_writer` function, but it requres a synchronous writer,
        // not a tokio `AsyncWrite`. Is there a way to bridge this without buffering the serialized
        // data in memory first?
        let data = serde_json::to_string(&notification).into_diagnostic()?;
        self.writer
            .write_all(data.as_bytes())
            .await
            .into_diagnostic()?;
        Ok(())
    }
}
