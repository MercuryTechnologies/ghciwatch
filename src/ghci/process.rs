use std::future::Future;
use std::pin::Pin;
use std::process::ExitStatus;

use command_group::AsyncGroupChild;
use miette::Context;
use miette::IntoDiagnostic;
use nix::sys::signal;
use nix::sys::signal::Signal;
use nix::unistd::Pid;
use tokio::sync::mpsc;
use tracing::instrument;

use crate::shutdown::ShutdownHandle;

pub struct GhciProcess {
    pub shutdown: ShutdownHandle,
    pub process_group_id: Pid,
    /// Notifies this task to _not_ request a shutdown for the entire program when `ghci` exits.
    /// This is used for the graceful shutdown implementation and for routine `ghci` session
    /// restarts.
    pub restart_receiver: mpsc::Receiver<()>,
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
        wait: Pin<&mut impl Future<Output = Result<ExitStatus, std::io::Error>>>,
    ) -> miette::Result<()> {
        // Kill it otherwise.
        tracing::debug!("Killing ghci process tree with SIGKILL");
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
        let status = wait.await.into_diagnostic()?;

        self.exited(status).await;
        Ok(())
    }

    async fn exited(&self, status: ExitStatus) {
        tracing::debug!("ghci exited: {status}");
    }
}
