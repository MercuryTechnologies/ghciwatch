use std::sync::Weak;

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
use tokio::sync::Mutex;
use tracing::instrument;

use super::Ghci;

/// An event sent to a `ghci` session's stderr channel.
#[derive(Debug)]
pub enum StderrEvent {
    /// Write to the `error_path` (`ghcid.txt`) file, if any.
    Write(oneshot::Sender<()>),
}

pub struct GhciStderr {
    pub ghci: Weak<Mutex<Ghci>>,
    pub reader: Lines<BufReader<ChildStderr>>,
    pub receiver: mpsc::Receiver<StderrEvent>,
    pub buffer: String,
    pub error_path: Option<Utf8PathBuf>,
}

impl GhciStderr {
    #[instrument(skip_all, name = "stderr", level = "debug")]
    pub async fn run(mut self) -> miette::Result<()> {
        loop {
            // TODO: Could this cause problems where we get an event and a final stderr line is only
            // processed after we write the error log?
            tokio::select! {
                Ok(Some(line)) = self.reader.next_line() => {
                    self.ingest_line(line).await;
                }
                Some(event) = self.receiver.recv() => {
                    match event {
                        StderrEvent::Write(sender) => {
                            let res = self.write().await;
                            let _ = sender.send(());
                            res?;
                        },
                    }
                }
            }
        }
    }

    #[instrument(skip(self), level = "debug")]
    async fn ingest_line(&mut self, line: String) {
        self.buffer.push_str(&line);
        self.buffer.push('\n');
        eprintln!("{line}");
    }

    #[instrument(skip_all, level = "debug")]
    async fn write(&mut self) -> miette::Result<()> {
        if self.buffer.is_empty() {
            // Nothing to do, don't wipe the error log file.
            tracing::debug!("Buffer empty, not writing");
            return Ok(());
        }

        if let Some(path) = &self.error_path {
            tracing::debug!(?path, bytes = self.buffer.len(), "Writing error log");
            let file = File::create(path).await.into_diagnostic()?;
            let mut writer = BufWriter::new(file);
            writer
                .write_all(&strip_ansi_escapes::strip(self.buffer.as_bytes()))
                .await
                .into_diagnostic()?;
            // This is load-bearing! If we don't properly flush/shutdown the handle, nothing gets
            // written!
            writer.shutdown().await.into_diagnostic()?;
        }

        self.buffer.clear();

        Ok(())
    }
}
