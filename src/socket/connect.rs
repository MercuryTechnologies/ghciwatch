use camino::Utf8Path;
use miette::Context;
use miette::IntoDiagnostic;
use tokio::net::UnixListener;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tracing::instrument;

use crate::runner::RunnerEvent;
use crate::socket::ServerRead;
use crate::socket::ServerWrite;

use super::ServerNotification;

/// Wraps a [`UnixListener`] and creates reader tasks corresponding to connections to a
/// socket.
pub struct SocketConnector {
    stream: UnixListener,
    sender: mpsc::Sender<RunnerEvent>,
    receiver: broadcast::Receiver<ServerNotification>,
}

impl SocketConnector {
    /// Create a new listener for the given socket path.
    pub fn new(
        path: &Utf8Path,
        sender: mpsc::Sender<RunnerEvent>,
        receiver: broadcast::Receiver<ServerNotification>,
    ) -> miette::Result<Self> {
        Ok(Self {
            stream: UnixListener::bind(path)
                .into_diagnostic()
                .wrap_err_with(|| format!("Failed to bind socket {path:?}"))?,
            sender,
            receiver,
        })
    }

    /// Run the task.
    #[instrument(skip_all, name = "socket-connector", level = "debug")]
    pub async fn run(mut self) -> miette::Result<()> {
        match self.run_inner().await {
            Ok(()) => {}
            Err(err) => {
                tracing::error!("{err:?}");
            }
        }

        Ok(())
    }

    async fn run_inner(&mut self) -> miette::Result<()> {
        let (stream, address) = self
            .stream
            .accept()
            .await
            .into_diagnostic()
            .wrap_err("Failed to accept a socket connection")?;

        tracing::debug!(?address, "Accepted a socket connection");

        let (read_half, write_half) = stream.into_split();

        let mut set = JoinSet::new();
        set.spawn(ServerRead::new(read_half, self.sender.clone()).run());
        // TODO: Resubscribing here might drop data. Should we empty the queue before this and
        // reinsert the values after resubscribing?
        set.spawn(ServerWrite::new(write_half, self.receiver.resubscribe()).run());

        // TODO: Allow multiple connections by storing the JoinSet somewhere?
        while let Some(result) = set.join_next().await {
            result
                .into_diagnostic()
                .wrap_err("Socket task panicked")?
                .wrap_err("Socket task failed")?;
        }

        Ok(())
    }
}
