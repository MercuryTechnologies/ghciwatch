use miette::IntoDiagnostic;
use serde::Deserialize;
use tokio::io::AsyncRead;
use tokio::sync::mpsc;
use tracing::instrument;

use crate::json::JsonReader;
use crate::runner::RunnerEvent;

/// A `ghcid-ng` server command. These are written to a socket to allow automating and scripting
/// `ghcid-ng`.
#[derive(Deserialize)]
pub enum ServerCommand {
    /// Quit the `ghci` session and exit `ghcid-ng`.
    Exit,
}

/// Task for reading input from a socket (in the form of [`ServerCommand`]s) to control `ghcid-ng`.
pub struct ServerRead<R> {
    /// The underlying reader.
    reader: JsonReader<ServerCommand, R>,
    /// A channel for communicating with a [`crate::runner::Runner`].
    sender: mpsc::Sender<RunnerEvent>,
}

impl<R> ServerRead<R>
where
    R: AsyncRead + Unpin,
{
    /// Create a new task to read commands to control `ghcid-ng`.
    pub fn new(reader: R, sender: mpsc::Sender<RunnerEvent>) -> Self {
        Self {
            reader: JsonReader::new(reader),
            sender,
        }
    }

    /// Run this reader task, sending events to the runner as they're received.
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
        while let Some(event) = self.reader.next().await? {
            match event {
                ServerCommand::Exit => {
                    self.send_exit().await?;
                }
            }
        }

        Ok(())
    }

    #[instrument(skip_all, level = "debug")]
    async fn send_exit(&mut self) -> miette::Result<()> {
        self.sender.send(RunnerEvent::Exit).await.into_diagnostic()
    }
}
