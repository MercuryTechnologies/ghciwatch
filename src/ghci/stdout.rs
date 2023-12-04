use aho_corasick::AhoCorasick;
use miette::Context;
use miette::IntoDiagnostic;
use tokio::io::Stdout;
use tokio::process::ChildStdout;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::instrument;

use crate::aho_corasick::AhoCorasickExt;
use crate::incremental_reader::FindAt;
use crate::incremental_reader::IncrementalReader;
use crate::incremental_reader::ReadOpts;
use crate::incremental_reader::WriteBehavior;

use super::parse::parse_ghc_messages;
use super::parse::parse_show_paths;
use super::parse::parse_show_targets;
use super::parse::ModuleSet;
use super::parse::ShowPaths;
use super::stderr::StderrEvent;
use super::CompilationLog;

pub struct GhciStdout {
    /// Reader for parsing and forwarding the underlying stdout stream.
    pub reader: IncrementalReader<ChildStdout, Stdout>,
    /// Channel for communicating with the stderr task.
    pub stderr_sender: mpsc::Sender<StderrEvent>,
    /// Prompt patterns to match. Constructing these `AhoCorasick` automatons is costly so we store
    /// them in the task state.
    pub prompt_patterns: AhoCorasick,
    /// A buffer to read data into. Lets us avoid allocating buffers in the [`IncrementalReader`].
    pub buffer: Vec<u8>,
}

impl GhciStdout {
    #[instrument(skip_all, level = "debug")]
    async fn parse_into_log(&self, data: &str, log: &mut CompilationLog) -> miette::Result<()> {
        // Parse GHCi output into compiler messages.
        //
        // These include diagnostics, which modules were compiled, and a compilation summary.
        let stderr_data = {
            let (sender, receiver) = oneshot::channel();
            let _ = self
                .stderr_sender
                .send(StderrEvent::GetBuffer { sender })
                .await;
            receiver.await.into_diagnostic()?
        };
        log.extend(parse_ghc_messages(&data).wrap_err("Failed to parse compiler output")?);
        log.extend(parse_ghc_messages(&stderr_data).wrap_err("Failed to parse compiler output")?);
        Ok(())
    }

    #[instrument(skip_all, name = "stdout_initialize", level = "debug")]
    pub async fn initialize(&mut self, log: &mut CompilationLog) -> miette::Result<()> {
        // Wait for `ghci` to start up. This may involve compiling a bunch of stuff.
        let bootup_patterns = AhoCorasick::from_anchored_patterns([
            "GHCi, version ",
            "GHCJSi, version ",
            "Clashi, version ",
        ]);
        let data = self
            .reader
            .read_until(&mut ReadOpts {
                end_marker: &bootup_patterns,
                find: FindAt::LineStart,
                writing: WriteBehavior::Write,
                buffer: &mut self.buffer,
            })
            .await?;
        tracing::debug!(data, "ghci started, saw version marker");

        self.parse_into_log(&data, log).await?;

        Ok(())
    }

    #[instrument(skip_all, level = "debug")]
    pub async fn prompt(&mut self, find: FindAt, log: &mut CompilationLog) -> miette::Result<()> {
        self.stderr_sender
            .send(StderrEvent::ClearBuffer)
            .await
            .into_diagnostic()?;

        let data = self
            .reader
            .read_until(&mut ReadOpts {
                end_marker: &self.prompt_patterns,
                find,
                writing: WriteBehavior::NoFinalLine,
                buffer: &mut self.buffer,
            })
            .await?;
        tracing::debug!(bytes = data.len(), "Got data from ghci");

        self.parse_into_log(&data, log).await?;
        Ok(())
    }

    #[instrument(skip_all, level = "debug")]
    pub async fn show_paths(&mut self) -> miette::Result<ShowPaths> {
        let lines = self
            .reader
            .read_until(&mut ReadOpts {
                end_marker: &self.prompt_patterns,
                find: FindAt::LineStart,
                writing: WriteBehavior::Hide,
                buffer: &mut self.buffer,
            })
            .await?;
        parse_show_paths(&lines).wrap_err("Failed to parse `:show paths` output")
    }

    #[instrument(skip_all, level = "debug")]
    pub async fn show_targets(&mut self, search_paths: &ShowPaths) -> miette::Result<ModuleSet> {
        let lines = self
            .reader
            .read_until(&mut ReadOpts {
                end_marker: &self.prompt_patterns,
                find: FindAt::LineStart,
                writing: WriteBehavior::Hide,
                buffer: &mut self.buffer,
            })
            .await?;
        let paths = parse_show_targets(search_paths, &lines)
            .wrap_err("Failed to parse `:show targets` output")?;
        ModuleSet::from_paths(paths, &search_paths.cwd)
    }

    #[instrument(skip_all, level = "debug")]
    pub async fn quit(&mut self) -> miette::Result<()> {
        let leaving_ghci = AhoCorasick::from_anchored_patterns(["Leaving GHCi."]);
        let data = self
            .reader
            .read_until(&mut ReadOpts {
                end_marker: &leaving_ghci,
                find: FindAt::Anywhere,
                writing: WriteBehavior::Write,
                buffer: &mut self.buffer,
            })
            .await?;
        tracing::debug!(data, "ghci confirmed quit on stdout");
        Ok(())
    }
}
