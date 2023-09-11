use aho_corasick::AhoCorasick;
use miette::Context;
use miette::IntoDiagnostic;
use tokio::io::Stdout;
use tokio::process::ChildStdout;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::instrument;

use crate::aho_corasick::AhoCorasickExt;
use crate::incremental_reader::IncrementalReader;
use crate::incremental_reader::WriteBehavior;
use crate::sync_sentinel::SyncSentinel;

use super::parse::parse_ghc_messages;
use super::parse::GhcMessage;
use super::parse::ModuleSet;
use super::stderr::StderrEvent;
use super::Mode;

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
    /// The mode we're currently reading output in.
    pub mode: Mode,
}

impl GhciStdout {
    #[instrument(skip_all, name = "stdout_initialize", level = "debug")]
    pub async fn initialize(&mut self) -> miette::Result<Vec<GhcMessage>> {
        // Wait for `ghci` to start up. This may involve compiling a bunch of stuff.
        let bootup_patterns = AhoCorasick::from_anchored_patterns([
            "GHCi, version ",
            "GHCJSi, version ",
            "Clashi, version ",
        ]);
        let data = self
            .reader
            .read_until(&bootup_patterns, WriteBehavior::Write, &mut self.buffer)
            .await?;
        tracing::debug!(data, "ghci started, saw version marker");

        // We know that we'll get _one_ `ghci> ` prompt on startup.
        let init_prompt_patterns = AhoCorasick::from_anchored_patterns(["ghci> "]);
        let messages = self.prompt(Some(&init_prompt_patterns)).await?;
        tracing::debug!("Saw initial `ghci> ` prompt");

        Ok(messages)
    }

    #[instrument(skip_all, level = "debug")]
    pub async fn prompt(
        &mut self,
        // We usually want this to be `&self.prompt_patterns`, but when we initialize we want to
        // pass in a different value. This method takes an `&mut self` reference, so if we try to
        // pass in `&self.prompt_patterns` when we call it we get a borrow error because the
        // compiler doesn't know we don't mess with `self.prompt_patterns` in here. So we use
        // `None` to represent that case and handle the default inline.
        prompt_patterns: Option<&AhoCorasick>,
    ) -> miette::Result<Vec<GhcMessage>> {
        let prompt_patterns = prompt_patterns.unwrap_or(&self.prompt_patterns);
        let data = self
            .reader
            .read_until(
                prompt_patterns,
                WriteBehavior::NoFinalLine,
                &mut self.buffer,
            )
            .await?;
        tracing::debug!(bytes = data.len(), "Got data from ghci");

        let result = if self.mode == Mode::Compiling {
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
            let mut messages =
                parse_ghc_messages(&data).wrap_err("Failed to parse compiler output")?;
            messages.extend(
                parse_ghc_messages(&stderr_data).wrap_err("Failed to parse compiler output")?,
            );
            messages
        } else {
            Vec::new()
        };

        Ok(result)
    }

    #[instrument(skip_all, level = "trace")]
    pub async fn sync(&mut self, sentinel: SyncSentinel) -> miette::Result<()> {
        // Read until the sync marker...
        let sync_pattern = AhoCorasick::from_anchored_patterns([sentinel.to_string()]);
        let data = self
            .reader
            .read_until(&sync_pattern, WriteBehavior::NoFinalLine, &mut self.buffer)
            .await?;
        // Then make sure to consume the prompt on the next line, and then we'll be caught up.
        let _ = self
            .reader
            .read_until(&self.prompt_patterns, WriteBehavior::Hide, &mut self.buffer)
            .await?;
        tracing::debug!(data, "Synced with ghci");

        // Tell the stderr stream to write the error log and then finish.
        let (err_sender, err_receiver) = oneshot::channel();
        let _ = self
            .stderr_sender
            .send(StderrEvent::Write(err_sender))
            .await;
        let _ = err_receiver.await;

        sentinel.finish();
        Ok(())
    }

    #[instrument(skip_all, level = "debug")]
    pub async fn show_modules(&mut self) -> miette::Result<ModuleSet> {
        let lines = self
            .reader
            .read_until(&self.prompt_patterns, WriteBehavior::Hide, &mut self.buffer)
            .await?;
        ModuleSet::from_lines(&lines)
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }
}
