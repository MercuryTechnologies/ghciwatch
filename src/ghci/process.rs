use std::process::ExitStatus;
use std::sync::Arc;

use miette::IntoDiagnostic;
use tokio::process::Child;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tracing::instrument;

use crate::shutdown::ShutdownHandle;

/// The state of a `ghci` process.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GhciProcessState {
    /// The process is still running.
    #[default]
    Running,
    /// The process has exited.
    Exited,
}

pub struct GhciProcess {
    pub shutdown: ShutdownHandle,
    pub process: Child,
    pub restart_receiver: mpsc::Receiver<()>,
    pub state: Arc<Mutex<GhciProcessState>>,
}

impl GhciProcess {
    #[instrument(skip_all, name = "ghci_process", level = "debug")]
    pub async fn run(mut self) -> miette::Result<()> {
        tokio::select! {
            _ = self.shutdown.on_shutdown_requested() => {
                self.stop().await?;
            }
            _ = self.restart_receiver.recv() => {
                tracing::debug!("ghci is being shut down");
            }
            result = self.process.wait() => {
                self.exited(result.into_diagnostic()?).await;
                let _ = self.shutdown.request_shutdown();
            }
        }
        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    async fn stop(&mut self) -> miette::Result<()> {
        // Give `ghci` a half second for a graceful shutdown.
        match tokio::time::timeout(std::time::Duration::from_millis(500), self.process.wait()).await
        {
            Ok(Ok(status)) => {
                self.exited(status).await;
                return Ok(());
            }
            Ok(Err(err)) => {
                tracing::debug!("Failed to wait for ghci: {err}");
            }
            Err(_) => {
                // Timeout expired.
                tracing::debug!("ghci didn't exit in time");
            }
        }

        // Kill it otherwise.
        self.process.kill().await.into_diagnostic()?;
        // Report the exit status.
        if let Some(status) = self.process.try_wait().into_diagnostic()? {
            self.exited(status).await;
        }
        Ok(())
    }

    async fn exited(&mut self, status: ExitStatus) {
        tracing::debug!("ghci exited: {status}");
        *self.state.lock().await = GhciProcessState::Exited;
    }
}
