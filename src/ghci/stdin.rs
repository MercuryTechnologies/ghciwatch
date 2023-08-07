use std::sync::Weak;

use camino::Utf8PathBuf;
use miette::IntoDiagnostic;
use tokio::io::AsyncWriteExt;
use tokio::process::ChildStdin;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tracing::instrument;

use crate::haskell_show::HaskellShow;
use crate::sync_sentinel::SyncSentinel;

use super::show_modules::ModuleSet;
use super::stdout::StdoutEvent;
use super::Ghci;
use super::IO_MODULE_NAME;
use super::PROMPT;

/// An event sent to a `ghci` session's stdin channel.
#[derive(Debug)]
pub enum StdinEvent {
    /// Initialize the `ghci` session; sets the initial imports, changes the prompt, etc.
    Initialize(oneshot::Sender<()>),
    /// Reload the `ghci` session with `:reload`.
    Reload(oneshot::Sender<()>),
    /// Add a module to the `ghci` session by path with `:load`.
    AddModule(Utf8PathBuf, oneshot::Sender<()>),
    /// Sync the `ghci` session's input/output.
    Sync(SyncSentinel),
    /// Show the currently loaded modules with `:show modules`.
    ShowModules(oneshot::Sender<ModuleSet>),
}

pub struct GhciStdin {
    pub ghci: Weak<Mutex<Ghci>>,
    pub stdin: ChildStdin,
    pub stdout_sender: mpsc::Sender<StdoutEvent>,
    pub receiver: mpsc::Receiver<StdinEvent>,
}

impl GhciStdin {
    #[instrument(skip_all, name = "stdin", level = "debug")]
    pub async fn run(mut self) -> miette::Result<()> {
        // For Stack, send a blank line to skip an initial prompt.
        //
        // See: https://github.com/ndmitchell/ghcid/issues/57
        self.stdin.write_all(b"\n").await.into_diagnostic()?;

        while let Some(event) = self.receiver.recv().await {
            match event {
                StdinEvent::Initialize(sender) => {
                    self.initialize(sender).await?;
                }
                StdinEvent::Reload(sender) => {
                    self.reload(sender).await?;
                }
                StdinEvent::AddModule(path, sender) => {
                    self.add_module(path, sender).await?;
                }
                StdinEvent::Sync(sentinel) => {
                    self.sync(sentinel).await?;
                }
                StdinEvent::ShowModules(sender) => {
                    self.show_modules(sender).await?;
                }
            }
        }

        Ok(())
    }

    /// Write a line on `stdin` and wait for a prompt on stdout.
    ///
    /// The `line` should contain the trailing newline.
    #[instrument(skip(self), level = "debug")]
    async fn write_line(&mut self, line: &str) -> miette::Result<()> {
        let (sender, receiver) = oneshot::channel();
        self.write_line_sender(line, sender).await?;
        receiver.await.into_diagnostic()?;
        Ok(())
    }

    /// Write a line on `stdin` and send an event to the given `sender` when a prompt is seen on
    /// stdout.
    ///
    /// The `line` should contain the trailing newline.
    #[instrument(skip(self), level = "debug")]
    async fn write_line_sender(
        &mut self,
        line: &str,
        sender: oneshot::Sender<()>,
    ) -> miette::Result<()> {
        self.stdin
            .write_all(line.as_bytes())
            .await
            .into_diagnostic()?;
        self.stdout_sender
            .send(StdoutEvent::Prompt(sender))
            .await
            .into_diagnostic()?;
        Ok(())
    }

    #[instrument(skip_all, level = "debug")]
    async fn initialize(&mut self, sender: oneshot::Sender<()>) -> miette::Result<()> {
        self.write_line(&format!(":set prompt {PROMPT}\n")).await?;
        self.write_line(&format!(":set prompt-cont {PROMPT}\n"))
            .await?;
        self.write_line(&format!("import qualified System.IO as {IO_MODULE_NAME}\n"))
            .await?;
        let _ = sender.send(());
        Ok(())
    }

    #[instrument(skip_all, level = "debug")]
    async fn reload(&mut self, sender: oneshot::Sender<()>) -> miette::Result<()> {
        self.write_line_sender(":reload\n", sender).await?;
        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    async fn add_module(
        &mut self,
        path: Utf8PathBuf,
        sender: oneshot::Sender<()>,
    ) -> miette::Result<()> {
        // We use `:load` here instead of `:add` because `:add` forces interpreted mode.
        self.write_line_sender(&format!(":load {path}\n"), sender)
            .await?;
        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    async fn sync(&mut self, sentinel: SyncSentinel) -> miette::Result<()> {
        self.stdin
            .write_all(
                format!("{IO_MODULE_NAME}.putStrLn {}\n", sentinel.haskell_show()).as_bytes(),
            )
            .await
            .into_diagnostic()?;

        self.stdout_sender
            .send(StdoutEvent::Sync(sentinel))
            .await
            .into_diagnostic()?;

        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    async fn show_modules(&mut self, sender: oneshot::Sender<ModuleSet>) -> miette::Result<()> {
        self.stdin
            .write_all(b":show modules\n")
            .await
            .into_diagnostic()?;

        self.stdout_sender
            .send(StdoutEvent::ShowModules(sender))
            .await
            .into_diagnostic()?;
        Ok(())
    }
}
