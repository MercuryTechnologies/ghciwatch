use std::net::Ipv4Addr;

use miette::Context;
use miette::IntoDiagnostic;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tracing::instrument;

use crate::runner::RunnerEvent;
use crate::server::ServerRead;
use crate::server::ServerWrite;

use super::ServerNotification;

/// Wraps a [`TcpListener`] and creates reader tasks corresponding to connections to a
/// port.
pub struct Server {
    listener: TcpListener,
    sender: mpsc::Sender<RunnerEvent>,
    receiver: broadcast::Receiver<ServerNotification>,
    connections: JoinSet<miette::Result<()>>,
}

impl Server {
    /// Create a new server binding to the given TCP port.
    pub async fn new(
        port: u16,
        sender: mpsc::Sender<RunnerEvent>,
        receiver: broadcast::Receiver<ServerNotification>,
    ) -> miette::Result<Self> {
        Ok(Self {
            listener: TcpListener::bind((Ipv4Addr::LOCALHOST, port))
                .await
                .into_diagnostic()
                .wrap_err_with(|| format!("Failed to bind TCP port {port}"))?,
            sender,
            receiver,
            connections: JoinSet::new(),
        })
    }

    /// Run the server.
    #[instrument(skip_all, name = "server", level = "debug")]
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
            .listener
            .accept()
            .await
            .into_diagnostic()
            .wrap_err("Failed to accept a TCP connection")?;

        tracing::debug!(?address, "Accepted a TCP connection");

        let (read_half, write_half) = stream.into_split();

        self.connections
            .spawn(ServerRead::new(read_half, self.sender.clone()).run());
        // TODO: Resubscribing here might drop data. Should we empty the queue before this and
        // reinsert the values after resubscribing?
        self.connections
            .spawn(ServerWrite::new(write_half, self.receiver.resubscribe()).run());

        Ok(())
    }
}
