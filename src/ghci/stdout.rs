use std::time::Duration;

use aho_corasick::AhoCorasick;
use eyre::Context;
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
use super::parse::ShowPaths;
use super::stderr::StderrEvent;
use super::writer::GhciWriter;
use super::CompilationLog;
use super::ModuleSet;

pub struct GhciStdout {
    /// Reader for parsing and forwarding the underlying stdout stream.
    pub reader: IncrementalReader<ChildStdout, GhciWriter>,
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
    async fn parse_into_log(&self, data: &str, log: &mut CompilationLog) -> eyre::Result<()> {
        // Parse GHCi output into compiler messages.
        //
        // These include diagnostics, which modules were compiled, and a compilation summary.
        let stderr_data = {
            let (sender, receiver) = oneshot::channel();
            let _ = self
                .stderr_sender
                .send(StderrEvent::GetBuffer { sender })
                .await;
            receiver.await?
        };
        log.extend(parse_ghc_messages(data).wrap_err("Failed to parse compiler output")?);
        log.extend(parse_ghc_messages(&stderr_data).wrap_err("Failed to parse compiler output")?);
        Ok(())
    }

    #[instrument(skip_all, name = "stdout_initialize", level = "debug")]
    pub async fn initialize(&mut self, log: &mut CompilationLog) -> eyre::Result<()> {
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
    pub async fn prompt(&mut self, find: FindAt, log: &mut CompilationLog) -> eyre::Result<()> {
        self.stderr_sender.send(StderrEvent::ClearBuffer).await?;

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

    /// Read any immediately-available output from the pipe, then drain stale prompts from
    /// the internal buffer. Returns the number of prompts found and discarded.
    pub async fn buffer_and_drain_prompts(&mut self, timeout: Duration) -> eyre::Result<usize> {
        self.reader
            .buffer_available(&mut self.buffer, timeout, WriteBehavior::NoFinalLine)
            .await?;

        self.reader
            .drain_buffered_chunks(&ReadOpts {
                end_marker: &self.prompt_patterns,
                find: FindAt::Anywhere,
                writing: WriteBehavior::NoFinalLine,
                buffer: &mut self.buffer,
            })
            .await
    }

    /// Read stdout until the given marker string is found, discarding everything before it.
    ///
    /// Used by `send_sigint` to synchronize with GHCi after an interrupt: a sync expression
    /// is sent on stdin and this method reads until its output appears, guaranteeing that all
    /// prior output has been consumed.
    pub async fn read_until_marker(&mut self, marker: &str) -> eyre::Result<String> {
        let pattern = AhoCorasick::from_anchored_patterns([marker]);
        self.reader
            .read_until(&mut ReadOpts {
                end_marker: &pattern,
                find: FindAt::Anywhere,
                writing: WriteBehavior::NoFinalLine,
                buffer: &mut self.buffer,
            })
            .await
    }

    #[instrument(skip_all, level = "debug")]
    pub async fn show_paths(&mut self) -> eyre::Result<ShowPaths> {
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
    pub async fn show_targets(&mut self, search_paths: &ShowPaths) -> eyre::Result<ModuleSet> {
        let lines = self
            .reader
            .read_until(&mut ReadOpts {
                end_marker: &self.prompt_patterns,
                find: FindAt::LineStart,
                writing: WriteBehavior::Hide,
                buffer: &mut self.buffer,
            })
            .await?;
        parse_show_targets(search_paths, &lines).wrap_err("Failed to parse `:show targets` output")
    }

    #[allow(dead_code)] // TODO: No it should not be!
    #[instrument(skip_all, level = "debug")]
    pub async fn quit(&mut self) -> eyre::Result<()> {
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
