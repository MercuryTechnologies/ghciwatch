use std::collections::BTreeMap;

use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use camino::Utf8PathBuf;
use miette::IntoDiagnostic;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::io::BufWriter;
use tokio::io::Lines;
use tokio::process::ChildStderr;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::instrument;

use super::Mode;

/// An event sent to a `ghci` session's stderr channel.
#[derive(Debug)]
pub enum StderrEvent {
    /// Write to the `error_path` (`ghcid.txt`) file, if any.
    Write(oneshot::Sender<()>),

    /// Set the compilation summary to the given string.
    SetCompilationSummary {
        summary: String,
        sender: oneshot::Sender<()>,
    },

    /// Set the writer's mode.
    Mode {
        mode: Mode,
        sender: oneshot::Sender<()>,
    },
}

pub struct GhciStderr {
    pub reader: Lines<BufReader<ChildStderr>>,
    pub receiver: mpsc::Receiver<StderrEvent>,
    /// A headline to write to the error log.
    ///
    /// This is a summary of the `ghci` session's status. Typically taken from compilation output,
    /// of the form `(Ok|Failed), [0-9]+ modules loaded.`.
    pub compilation_summary: String,
    /// Output buffers, one per [`Mode`]. This lets us gather output for compilation and tests
    /// separately. Useful to avoid clobbering the `error_path` with test data when there were
    /// useful compilation errors stored.
    pub buffers: BTreeMap<Mode, String>,
    /// The path to write the error log to.
    pub error_path: Option<Utf8PathBuf>,
    /// The mode we're currently reading output in.
    pub mode: Mode,
    /// `true` if we've read data from stderr but not yet written it to the error log.
    pub has_unwritten_data: bool,
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

    async fn run_inner(&mut self) -> miette::Result<()> {
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
            StderrEvent::Write(sender) => {
                let res = self.write().await;
                let _ = sender.send(());
                res?;
            }
            StderrEvent::Mode { mode, sender } => {
                self.set_mode(sender, mode).await;
            }
            StderrEvent::SetCompilationSummary { summary, sender } => {
                self.set_compilation_summary(sender, summary).await;
            }
        }

        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    async fn ingest_line(&mut self, line: String) {
        // We might not have a buffer for some modes, e.g. `Internal`.
        if let Some(buffer) = self.buffers.get_mut(&self.mode) {
            buffer.push_str(&line);
            buffer.push('\n');
            self.has_unwritten_data = true;
        }
        eprintln!("{line}");
    }

    #[instrument(skip_all, level = "debug")]
    async fn write(&mut self) -> miette::Result<()> {
        if !self.has_unwritten_data {
            tracing::debug!("No new data, not writing");
            return Ok(());
        }

        if let Some(path) = &self.error_path {
            let file = File::create(path).await.into_diagnostic()?;
            let mut writer = BufWriter::new(file);

            if !self.compilation_summary.is_empty() {
                tracing::debug!(?path, "Writing error log headline");
                writer
                    .write_all(self.compilation_summary.as_bytes())
                    .await
                    .into_diagnostic()?;
            }

            for (mode, buffer) in &self.buffers {
                tracing::debug!(?path, %mode, bytes = buffer.len(), "Writing error log");
                writer
                    .write_all(&strip_ansi_escapes::strip(buffer.as_bytes()))
                    .await
                    .into_diagnostic()?;
            }

            // This is load-bearing! If we don't properly flush/shutdown the handle, nothing gets
            // written!
            writer.shutdown().await.into_diagnostic()?;
        }

        self.has_unwritten_data = false;

        Ok(())
    }

    #[instrument(skip(self, sender), level = "debug")]
    async fn set_mode(&mut self, sender: oneshot::Sender<()>, mode: Mode) {
        self.mode = mode;

        // Clear the buffer for the newly-selected mode.
        //
        // TODO: What if this deletes useful errors that don't get resurfaced?
        // For example, a user adds a new module and `ghci` shows errors with that module but not
        // previous compilation errors in different modules.
        // Maybe analagous to the way warnings can get hidden because they're only surfaced during
        // compilation of that particular module and aren't fatal.
        if let Some(buffer) = self.buffers.get_mut(&self.mode) {
            buffer.clear();
        }

        // If we're compiling, also clear the headline so we don't write a stale status/module
        // count.
        if mode == Mode::Compiling {
            self.compilation_summary.clear();
        }

        let _ = sender.send(());
    }

    #[instrument(skip(self, sender), level = "debug")]
    async fn set_compilation_summary(&mut self, sender: oneshot::Sender<()>, summary: String) {
        if summary != self.compilation_summary {
            self.compilation_summary = summary;
            self.has_unwritten_data = true;
        }
        let _ = sender.send(());
    }
}
