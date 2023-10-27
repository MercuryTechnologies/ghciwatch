use std::future::Future;
use std::pin::Pin;
use std::process::ExitStatus;
use std::sync::Arc;
use std::time::Duration;

use command_group::AsyncGroupChild;
use miette::Context;
use miette::IntoDiagnostic;
use nix::sys::signal;
use nix::sys::signal::Signal;
use nix::unistd::Pid;
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
    pub process_group_id: Pid,
    pub restart_receiver: mpsc::Receiver<()>,
    pub state: Arc<Mutex<GhciProcessState>>,
}

impl GhciProcess {
    #[instrument(skip_all, name = "ghci_process", level = "debug")]
    pub async fn run(mut self, mut process: AsyncGroupChild) -> miette::Result<()> {
        // We can only call `wait()` once at a time, so we store the future and pass it into the
        // `stop()` handler.
        let mut wait = std::pin::pin!(process.wait());
        tokio::select! {
            _ = self.shutdown.on_shutdown_requested() => {
                self.stop(wait).await?;
            }
            _ = self.restart_receiver.recv() => {
                tracing::debug!("ghci is being shut down");
                self.stop(wait).await?;
            }
            result = &mut wait => {
                self.exited(result.into_diagnostic()?).await;
                let _ = self.shutdown.request_shutdown();
            }
        }
        Ok(())
    }

    #[instrument(skip_all, level = "debug")]
    async fn stop(
        &self,
        mut wait: Pin<&mut impl Future<Output = Result<ExitStatus, std::io::Error>>>,
    ) -> miette::Result<()> {
        let status = async {
            // Give `ghci` a bit for a graceful shutdown.
            match tokio::time::timeout(Duration::from_secs(10), &mut wait).await {
                Ok(Ok(status)) => {
                    return Ok(status);
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
            tracing::debug!("Killing ghci ungracefully");
            // This is what `self.process.kill()` does, but we can't call that due to borrow
            // checker shennanigans.
            signal::killpg(self.process_group_id, Signal::SIGKILL)
                .into_diagnostic()
                .wrap_err_with(|| {
                    format!(
                        "Failed to kill ghci process (pid {})",
                        self.process_group_id
                    )
                })?;
            // Report the exit status.
            wait.await.into_diagnostic()
        }
        .await?;

        self.exited(status).await;
        Ok(())
    }

    async fn exited(&self, status: ExitStatus) {
        tracing::debug!("ghci exited: {status}");
        *self.state.lock().await = GhciProcessState::Exited;
    }
}
