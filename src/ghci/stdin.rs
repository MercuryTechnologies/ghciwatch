use camino::Utf8Path;
use miette::Context;
use miette::IntoDiagnostic;
use tokio::io::AsyncWriteExt;
use tokio::process::ChildStdin;
use tokio::sync::mpsc;
use tracing::instrument;

use crate::incremental_reader::FindAt;

use super::parse::GhcMessage;
use super::parse::ModuleSet;
use super::parse::ShowPaths;
use super::stderr::StderrEvent;
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
    async fn write_line_with_prompt_at(
        &mut self,
        stdout: &mut GhciStdout,
        line: &str,
        find: FindAt,
    ) -> miette::Result<Vec<GhcMessage>> {
        self.stdin
            .write_all(line.as_bytes())
            .await
            .into_diagnostic()?;
        stdout.prompt(find).await
    }

    /// Write a line on `stdin` and wait for a prompt on stdout.
    ///
    /// The `line` should contain the trailing newline.
    async fn write_line(
        &mut self,
        stdout: &mut GhciStdout,
        line: &str,
    ) -> miette::Result<Vec<GhcMessage>> {
        self.write_line_with_prompt_at(stdout, line, FindAt::LineStart)
            .await
    }

    /// Run a [`GhciCommand`].
    ///
    /// The command may be multiple lines.
    #[instrument(skip(self, stdout), level = "debug")]
    pub async fn run_command(
        &mut self,
        stdout: &mut GhciStdout,
        command: &GhciCommand,
    ) -> miette::Result<Vec<GhcMessage>> {
        let mut messages = Vec::new();

        for line in command.lines() {
            self.stdin
                .write_all(line.as_bytes())
                .await
                .into_diagnostic()?;
            self.stdin.write_all(b"\n").await.into_diagnostic()?;
            messages.extend(stdout.prompt(FindAt::LineStart).await?);
        }

        Ok(messages)
    }

    #[instrument(skip(self, stdout), name = "stdin_initialize", level = "debug")]
    pub async fn initialize(&mut self, stdout: &mut GhciStdout) -> miette::Result<Vec<GhcMessage>> {
        // We tell stdout/stderr we're compiling for the first prompt because this includes all the
        // module compilation before the first prompt.
        let messages = self
            .write_line_with_prompt_at(stdout, &format!(":set prompt {PROMPT}\n"), FindAt::Anywhere)
            .await?;
        self.write_line(stdout, &format!(":set prompt-cont {PROMPT}\n"))
            .await?;

        Ok(messages)
    }

    #[instrument(skip_all, level = "debug")]
    pub async fn reload(&mut self, stdout: &mut GhciStdout) -> miette::Result<Vec<GhcMessage>> {
        self.write_line(stdout, ":reload\n").await
    }

    #[instrument(skip(self, stdout), level = "debug")]
    pub async fn add_module(
        &mut self,
        stdout: &mut GhciStdout,
        path: &Utf8Path,
    ) -> miette::Result<Vec<GhcMessage>> {
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
    pub async fn interpret_module(
        &mut self,
        stdout: &mut GhciStdout,
        path: &Utf8Path,
    ) -> miette::Result<Vec<GhcMessage>> {
        // `:add *` forces the module to be interpreted, even if it was already loaded from
        // bytecode. This is necessary to access the module's top-level binds for the eval feature.
        self.write_line(stdout, &format!(":add *{path}\n")).await
    }

    #[instrument(skip(self, stdout), level = "debug")]
    pub async fn show_paths(&mut self, stdout: &mut GhciStdout) -> miette::Result<ShowPaths> {
        self.stdin
            .write_all(b":show paths\n")
            .await
            .into_diagnostic()?;

        stdout.show_paths().await
    }

    #[instrument(skip_all, level = "debug")]
    pub async fn show_targets(
        &mut self,
        stdout: &mut GhciStdout,
        show_paths: &ShowPaths,
    ) -> miette::Result<ModuleSet> {
        self.stdin
            .write_all(b":show targets\n")
            .await
            .into_diagnostic()?;

        stdout.show_targets(show_paths).await
    }

    #[instrument(skip(self, stdout), level = "debug")]
    pub async fn quit(&mut self, stdout: &mut GhciStdout) -> miette::Result<()> {
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

    #[instrument(skip(self, stdout), level = "debug")]
    pub async fn eval(
        &mut self,
        stdout: &mut GhciStdout,
        module_name: &str,
        command: &GhciCommand,
    ) -> miette::Result<Vec<GhcMessage>> {
        let mut messages = Vec::new();

        messages.extend(
            self.write_line(stdout, &format!(":module + *{module_name}\n"))
                .await?,
        );

        messages.extend(self.run_command(stdout, command).await?);

        messages.extend(
            self.write_line(stdout, &format!(":module - *{module_name}\n"))
                .await?,
        );

        Ok(messages)
    }
}
