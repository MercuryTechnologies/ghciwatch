use std::time::Instant;

use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use camino::Utf8PathBuf;
use miette::IntoDiagnostic;
use tokio::io::AsyncWriteExt;
use tokio::process::ChildStdin;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use tracing::instrument;

use crate::haskell_show::HaskellShow;
use crate::sync_sentinel::SyncSentinel;

use super::show_modules::ModuleSet;
use super::stderr::StderrEvent;
use super::stdout::StdoutEvent;
use super::Mode;
use super::IO_MODULE_NAME;
use super::PROMPT;

/// An event sent to a `ghci` session's stdin channel.
#[derive(Debug)]
pub enum StdinEvent {
    /// Initialize the `ghci` session; sets the initial imports, changes the prompt, etc.
    Initialize {
        sender: oneshot::Sender<()>,
        setup_commands: Vec<String>,
    },
    /// Reload the `ghci` session with `:reload`.
    Reload(oneshot::Sender<Option<Result<(), ()>>>),
    /// Run the user-provided test command, if any.
    Test {
        sender: oneshot::Sender<()>,
        test_command: Option<String>,
    },
    /// Add a module to the `ghci` session by path with `:add`.
    AddModule(Utf8PathBuf, oneshot::Sender<Option<Result<(), ()>>>),
    /// Sync the `ghci` session's input/output.
    Sync(SyncSentinel),
    /// Show the currently loaded modules with `:show modules`.
    ShowModules(oneshot::Sender<ModuleSet>),
}

pub struct GhciStdin {
    /// Inner stdin writer.
    pub stdin: ChildStdin,
    /// Channel sender for communicating with the stdout task.
    pub stdout_sender: mpsc::Sender<StdoutEvent>,
    /// Channel sender for communicating with the stderr task.
    pub stderr_sender: mpsc::Sender<StderrEvent>,
    /// Channel receiver for communicating with this task.
    pub receiver: mpsc::Receiver<StdinEvent>,
}

impl GhciStdin {
    #[instrument(skip_all, name = "stdin", level = "debug")]
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

    pub async fn run_inner(&mut self) -> miette::Result<()> {
        while let Some(event) = self.receiver.recv().await {
            match event {
                StdinEvent::Initialize {
                    sender,
                    setup_commands,
                } => {
                    self.initialize(sender, setup_commands).await?;
                }
                StdinEvent::Reload(sender) => {
                    self.reload(sender).await?;
                }
                StdinEvent::Test {
                    sender,
                    test_command,
                } => {
                    self.test(sender, test_command).await?;
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
        sender: oneshot::Sender<Option<Result<(), ()>>>,
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
    async fn initialize(
        &mut self,
        sender: oneshot::Sender<()>,
        setup_commands: Vec<String>,
    ) -> miette::Result<()> {
        self.set_mode(Mode::Internal).await?;
        self.write_line(&format!(":set prompt {PROMPT}\n")).await?;
        self.write_line(&format!(":set prompt-cont {PROMPT}\n"))
            .await?;
        self.write_line(&format!("import qualified System.IO as {IO_MODULE_NAME}\n"))
            .await?;

        for command in setup_commands {
            tracing::debug!(?command, "Running user intialization command");
            self.write_line(&format!("{command}\n")).await?;
        }

        let _ = sender.send(());
        Ok(())
    }

    #[instrument(skip_all, level = "debug")]
    async fn reload(
        &mut self,
        sender: oneshot::Sender<Option<Result<(), ()>>>,
    ) -> miette::Result<()> {
        self.set_mode(Mode::Compiling).await?;
        self.write_line_sender(":reload\n", sender).await?;
        Ok(())
    }

    #[instrument(skip_all, level = "debug")]
    async fn test(
        &mut self,
        sender: oneshot::Sender<()>,
        test_command: Option<String>,
    ) -> miette::Result<()> {
        if let Some(test_command) = test_command {
            self.set_mode(Mode::Testing).await?;
            tracing::debug!(command = ?test_command, "Running user test command");
            tracing::info!("Running tests");
            let start_time = Instant::now();
            self.write_line(&format!("{test_command}\n")).await?;
            tracing::info!("Finished running tests in {:.2?}", start_time.elapsed());
        }

        let _ = sender.send(());

        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    async fn add_module(
        &mut self,
        path: Utf8PathBuf,
        sender: oneshot::Sender<Option<Result<(), ()>>>,
    ) -> miette::Result<()> {
        self.set_mode(Mode::Compiling).await?;

        // We use `:add` because `:load` unloads all previously loaded modules:
        //
        // > All previously loaded modules, except package modules, are forgotten. The new set of
        // > modules is known as the target set. Note that :load can be used without any arguments
        // > to unload all the currently loaded modules and bindings.
        //
        // https://downloads.haskell.org/ghc/latest/docs/users_guide/ghci.html#ghci-cmd-:load
        self.write_line_sender(&format!(":add {path}\n"), sender)
            .await?;
        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    async fn sync(&mut self, sentinel: SyncSentinel) -> miette::Result<()> {
        self.set_mode(Mode::Internal).await?;

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
        self.set_mode(Mode::Internal).await?;

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

    #[instrument(skip(self), level = "debug")]
    async fn set_mode(&self, mode: Mode) -> miette::Result<()> {
        let mut set = JoinSet::<Result<(), oneshot::error::RecvError>>::new();

        {
            let (sender, receiver) = oneshot::channel();
            self.stdout_sender
                .send(StdoutEvent::Mode { mode, sender })
                .await
                .into_diagnostic()?;
            set.spawn(receiver);
        }

        {
            let (sender, receiver) = oneshot::channel();
            self.stderr_sender
                .send(StderrEvent::Mode { mode, sender })
                .await
                .into_diagnostic()?;
            set.spawn(receiver);
        }

        // Wait until the other tasks have finished setting the new mode.
        while let Some(result) = set.join_next().await {
            result.into_diagnostic()?.into_diagnostic()?;
        }

        Ok(())
    }
}
