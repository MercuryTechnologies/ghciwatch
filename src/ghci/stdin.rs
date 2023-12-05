use camino::Utf8Path;
use miette::Context;
use miette::IntoDiagnostic;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;
use tokio::process::ChildStdin;
use tokio::sync::mpsc;
use tracing::instrument;

use crate::incremental_reader::FindAt;

use super::parse::ModuleSet;
use super::parse::ShowPaths;
use super::stderr::StderrEvent;
use super::CompilationLog;
use super::GhciCommand;
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
    ///
    /// The `find` parameter determines where the prompt can be found in the output line.
    #[instrument(skip(self, stdout), level = "debug")]
    async fn write_line_with_prompt_at<W>(
        &mut self,
        stdout: &mut GhciStdout<W>,
        line: &str,
        find: FindAt,
        log: &mut CompilationLog,
    ) -> miette::Result<()>
    where
        W: AsyncWrite,
    {
        self.stdin
            .write_all(line.as_bytes())
            .await
            .into_diagnostic()?;
        stdout.prompt(find, log).await
    }

    /// Write a line on `stdin` and wait for a prompt on stdout.
    ///
    /// The `line` should contain the trailing newline.
    async fn write_line<W>(
        &mut self,
        stdout: &mut GhciStdout<W>,
        line: &str,
        log: &mut CompilationLog,
    ) -> miette::Result<()>
    where
        W: AsyncWrite,
    {
        self.write_line_with_prompt_at(stdout, line, FindAt::LineStart, log)
            .await
    }

    /// Run a [`GhciCommand`].
    ///
    /// The command may be multiple lines.
    #[instrument(skip(self, stdout), level = "debug")]
    pub async fn run_command<W>(
        &mut self,
        stdout: &mut GhciStdout<W>,
        command: &GhciCommand,
        log: &mut CompilationLog,
    ) -> miette::Result<()>
    where
        W: AsyncWrite,
    {
        for line in command.lines() {
            self.write_line(stdout, &format!("{line}\n"), log).await?;
        }

        Ok(())
    }

    #[instrument(skip(self, stdout), name = "stdin_initialize", level = "debug")]
    pub async fn initialize<W>(
        &mut self,
        stdout: &mut GhciStdout<W>,
        log: &mut CompilationLog,
    ) -> miette::Result<()>
    where
        W: AsyncWrite,
    {
        // We tell stdout/stderr we're compiling for the first prompt because this includes all the
        // module compilation before the first prompt.
        self.write_line_with_prompt_at(
            stdout,
            &format!(":set prompt {PROMPT}\n"),
            FindAt::Anywhere,
            log,
        )
        .await?;
        self.write_line(stdout, &format!(":set prompt-cont {PROMPT}\n"), log)
            .await?;
        Ok(())
    }

    #[instrument(skip_all, level = "debug")]
    pub async fn reload<W>(
        &mut self,
        stdout: &mut GhciStdout<W>,
        log: &mut CompilationLog,
    ) -> miette::Result<()>
    where
        W: AsyncWrite,
    {
        self.write_line(stdout, ":reload\n", log).await
    }

    #[instrument(skip(self, stdout), level = "debug")]
    pub async fn add_module<W>(
        &mut self,
        stdout: &mut GhciStdout<W>,
        path: &Utf8Path,
        log: &mut CompilationLog,
    ) -> miette::Result<()>
    where
        W: AsyncWrite,
    {
        // We use `:add` because `:load` unloads all previously loaded modules:
        //
        // > All previously loaded modules, except package modules, are forgotten. The new set of
        // > modules is known as the target set. Note that :load can be used without any arguments
        // > to unload all the currently loaded modules and bindings.
        //
        // https://downloads.haskell.org/ghc/latest/docs/users_guide/ghci.html#ghci-cmd-:load
        self.write_line(stdout, &format!(":add {path}\n"), log)
            .await
    }

    #[instrument(skip(self, stdout), level = "debug")]
    pub async fn interpret_module<W>(
        &mut self,
        stdout: &mut GhciStdout<W>,
        path: &Utf8Path,
        log: &mut CompilationLog,
    ) -> miette::Result<()>
    where
        W: AsyncWrite,
    {
        // `:add *` forces the module to be interpreted, even if it was already loaded from
        // bytecode. This is necessary to access the module's top-level binds for the eval feature.
        self.write_line(stdout, &format!(":add *{path}\n"), log)
            .await
    }

    #[instrument(skip(self, stdout), level = "debug")]
    pub async fn eval<W>(
        &mut self,
        stdout: &mut GhciStdout<W>,
        module_name: &str,
        command: &GhciCommand,
        log: &mut CompilationLog,
    ) -> miette::Result<()>
    where
        W: AsyncWrite,
    {
        self.write_line(stdout, &format!(":module + *{module_name}\n"), log)
            .await?;

        self.run_command(stdout, command, log).await?;

        self.write_line(stdout, &format!(":module - *{module_name}\n"), log)
            .await?;

        Ok(())
    }

    #[instrument(skip(self, stdout), level = "debug")]
    pub async fn show_paths<W>(&mut self, stdout: &mut GhciStdout<W>) -> miette::Result<ShowPaths>
    where
        W: AsyncWrite,
    {
        self.stdin
            .write_all(b":show paths\n")
            .await
            .into_diagnostic()?;

        stdout.show_paths().await
    }

    #[instrument(skip_all, level = "debug")]
    pub async fn show_targets<W>(
        &mut self,
        stdout: &mut GhciStdout<W>,
        show_paths: &ShowPaths,
    ) -> miette::Result<ModuleSet>
    where
        W: AsyncWrite,
    {
        self.stdin
            .write_all(b":show targets\n")
            .await
            .into_diagnostic()?;

        stdout.show_targets(show_paths).await
    }

    #[instrument(skip(self, stdout), level = "debug")]
    pub async fn quit<W>(&mut self, stdout: &mut GhciStdout<W>) -> miette::Result<()>
    where
        W: AsyncWrite,
    {
        self.stdin
            .write_all(b":quit\n")
            .await
            .into_diagnostic()
            .wrap_err("Failed to tell ghci to `:quit`")?;
        stdout
            .quit()
            .await
            .wrap_err("Failed to wait for ghci to quit")
    }
}
