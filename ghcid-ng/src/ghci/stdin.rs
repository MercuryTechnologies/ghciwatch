use std::time::Instant;

use camino::Utf8Path;
use miette::IntoDiagnostic;
use tokio::io::AsyncWriteExt;
use tokio::process::ChildStdin;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use tracing::instrument;

use crate::haskell_show::HaskellShow;
use crate::sync_sentinel::SyncSentinel;

use super::parse::GhcMessage;
use super::parse::ModuleSet;
use super::stderr::StderrEvent;
use super::Mode;
use super::IO_MODULE_NAME;
use super::PROMPT;
use crate::ghci::GhciStdout;

pub struct GhciStdin {
    /// Inner stdin writer.
    pub stdin: ChildStdin,
    /// Channel sender for communicating with the stderr task.
    pub stderr_sender: mpsc::Sender<StderrEvent>,
}

impl GhciStdin {
    /// Write a line on `stdin` and wait for a prompt on stdout.
    ///
    /// The `line` should contain the trailing newline.
    #[instrument(skip(self, stdout), level = "debug")]
    pub async fn write_line(
        &mut self,
        stdout: &mut GhciStdout,
        line: &str,
    ) -> miette::Result<Vec<GhcMessage>> {
        self.stdin
            .write_all(line.as_bytes())
            .await
            .into_diagnostic()?;
        stdout.prompt(None).await
    }

    #[instrument(skip(self, stdout), level = "debug")]
    pub async fn initialize(
        &mut self,
        stdout: &mut GhciStdout,
        setup_commands: &[String],
    ) -> miette::Result<()> {
        self.set_mode(stdout, Mode::Internal).await?;
        self.write_line(stdout, &format!(":set prompt {PROMPT}\n"))
            .await?;
        self.write_line(stdout, &format!(":set prompt-cont {PROMPT}\n"))
            .await?;
        self.write_line(
            stdout,
            &format!("import qualified System.IO as {IO_MODULE_NAME}\n"),
        )
        .await?;

        for command in setup_commands {
            tracing::debug!(?command, "Running user intialization command");
            self.write_line(stdout, &format!("{command}\n")).await?;
        }

        Ok(())
    }

    #[instrument(skip_all, level = "debug")]
    pub async fn reload(&mut self, stdout: &mut GhciStdout) -> miette::Result<Vec<GhcMessage>> {
        self.set_mode(stdout, Mode::Compiling).await?;
        self.write_line(stdout, ":reload\n").await
    }

    #[instrument(skip_all, level = "debug")]
    pub async fn test(
        &mut self,
        stdout: &mut GhciStdout,
        test_command: Option<String>,
    ) -> miette::Result<()> {
        if let Some(test_command) = test_command {
            self.set_mode(stdout, Mode::Testing).await?;
            tracing::debug!(command = ?test_command, "Running user test command");
            tracing::info!("Running tests");
            let start_time = Instant::now();
            self.write_line(stdout, &format!("{test_command}\n"))
                .await?;
            tracing::info!("Finished running tests in {:.2?}", start_time.elapsed());
        } else {
            tracing::debug!("No test command provided, not running tests");
        }

        Ok(())
    }

    #[instrument(skip(self, stdout), level = "debug")]
    pub async fn add_module(
        &mut self,
        stdout: &mut GhciStdout,
        path: &Utf8Path,
    ) -> miette::Result<Vec<GhcMessage>> {
        self.set_mode(stdout, Mode::Compiling).await?;

        // We use `:add` because `:load` unloads all previously loaded modules:
        //
        // > All previously loaded modules, except package modules, are forgotten. The new set of
        // > modules is known as the target set. Note that :load can be used without any arguments
        // > to unload all the currently loaded modules and bindings.
        //
        // https://downloads.haskell.org/ghc/latest/docs/users_guide/ghci.html#ghci-cmd-:load
        self.write_line(stdout, &format!(":add {path}\n")).await
    }

    #[instrument(skip(self, stdout), level = "debug")]
    pub async fn sync(
        &mut self,
        stdout: &mut GhciStdout,
        sentinel: SyncSentinel,
    ) -> miette::Result<()> {
        self.set_mode(stdout, Mode::Internal).await?;

        self.stdin
            .write_all(
                format!("{IO_MODULE_NAME}.putStrLn {}\n", sentinel.haskell_show()).as_bytes(),
            )
            .await
            .into_diagnostic()?;

        stdout.sync(sentinel).await?;

        Ok(())
    }

    #[instrument(skip(self, stdout), level = "debug")]
    pub async fn show_modules(&mut self, stdout: &mut GhciStdout) -> miette::Result<ModuleSet> {
        self.set_mode(stdout, Mode::Internal).await?;

        self.stdin
            .write_all(b":show modules\n")
            .await
            .into_diagnostic()?;

        stdout.show_modules().await
    }

    #[instrument(skip(self, stdout), level = "debug")]
    pub async fn set_mode(&mut self, stdout: &mut GhciStdout, mode: Mode) -> miette::Result<()> {
        let mut set = JoinSet::<Result<(), oneshot::error::RecvError>>::new();

        stdout.set_mode(mode);

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
