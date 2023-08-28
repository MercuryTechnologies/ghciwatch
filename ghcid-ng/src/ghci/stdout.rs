use std::sync::OnceLock;

use aho_corasick::AhoCorasick;
use miette::Context;
use miette::IntoDiagnostic;
use regex::Regex;
use tokio::io::Stdout;
use tokio::process::ChildStdout;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::instrument;

use crate::aho_corasick::AhoCorasickExt;
use crate::incremental_reader::IncrementalReader;
use crate::incremental_reader::WriteBehavior;
use crate::lines::Lines;
use crate::sync_sentinel::SyncSentinel;

use super::show_modules::ModuleSet;
use super::stderr::StderrEvent;
use super::CompilationResult;
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
    #[instrument(skip_all, level = "debug")]
    pub async fn initialize(&mut self) -> miette::Result<()> {
        // Wait for `ghci` to start up. This may involve compiling a bunch of stuff.
        let bootup_patterns = AhoCorasick::from_anchored_patterns([
            "GHCi, version ",
            "GHCJSi, version ",
            "Clashi, version ",
        ]);
        let lines = self
            .reader
            .read_until(&bootup_patterns, WriteBehavior::Write, &mut self.buffer)
            .await?;
        tracing::debug!(?lines, "ghci started, saw version marker");

        // We know that we'll get _one_ `ghci> ` prompt on startup.
        let init_prompt_patterns = AhoCorasick::from_anchored_patterns(["ghci> "]);
        self.prompt(Some(&init_prompt_patterns)).await?;
        tracing::debug!("Saw initial `ghci> ` prompt");

        Ok(())
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
    ) -> miette::Result<Option<CompilationResult>> {
        let prompt_patterns = prompt_patterns.unwrap_or(&self.prompt_patterns);
        let lines = self
            .reader
            .read_until(
                prompt_patterns,
                WriteBehavior::NoFinalLine,
                &mut self.buffer,
            )
            .await?;
        tracing::debug!(lines = lines.len(), "Got data from ghci");

        let mut result = None;
        if self.mode == Mode::Compiling {
            result = self
                .get_status_from_compile_output(lines)
                .await
                .wrap_err("Failed to get status from compilation output")?;
        }

        // Tell the stderr stream to write the error log and then finish.
        {
            let (sender, receiver) = oneshot::channel();
            self.stderr_sender
                .send(StderrEvent::Write(sender))
                .await
                .into_diagnostic()?;
            receiver.await.into_diagnostic()?;
        }

        Ok(result)
    }

    #[instrument(skip_all, level = "debug")]
    pub async fn sync(&mut self, sentinel: SyncSentinel) -> miette::Result<()> {
        // Read until the sync marker...
        let sync_pattern = AhoCorasick::from_anchored_patterns([sentinel.to_string()]);
        let lines = self
            .reader
            .read_until(&sync_pattern, WriteBehavior::NoFinalLine, &mut self.buffer)
            .await?;
        // Then make sure to consume the prompt on the next line, and then we'll be caught up.
        let _ = self
            .reader
            .read_until(&self.prompt_patterns, WriteBehavior::Hide, &mut self.buffer)
            .await?;
        tracing::debug!(?lines, "Got data from ghci");

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

    #[instrument(skip(self), level = "debug")]
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }

    /// Get the compilation status from a chunk of lines. The compilation status is on the last
    /// line.
    ///
    /// The outer result fails if any of the operation failed, the inner `Option` conveys the
    /// compilation status.
    async fn get_status_from_compile_output(
        &mut self,
        mut lines: Lines,
    ) -> miette::Result<Option<CompilationResult>> {
        if let Some(line) = lines.pop() {
            if compilation_finished_re().is_match(&line) {
                let result = if line.starts_with("Ok") {
                    tracing::debug!("Compilation succeeded");
                    CompilationResult::Ok
                } else {
                    tracing::debug!("Compilation failed");
                    CompilationResult::Err
                };

                let (sender, receiver) = oneshot::channel();
                self.stderr_sender
                    .send(StderrEvent::SetCompilationSummary {
                        summary: line,
                        sender,
                    })
                    .await
                    .into_diagnostic()?;
                receiver.await.into_diagnostic()?;

                return Ok(Some(result));
            } else {
                tracing::debug!("Didn't parse 'modules loaded' line");
            }
        }

        Ok(None)
    }
}

fn compilation_finished_re() -> &'static Regex {
    // There's special cases for 0-6 modules!
    // https://gitlab.haskell.org/ghc/ghc/-/blob/288235bbe5a59b8a1bda80aaacd59e5717417726/ghc/GHCi/UI.hs#L2286-L2287
    // https://gitlab.haskell.org/ghc/ghc/-/blob/288235bbe5a59b8a1bda80aaacd59e5717417726/compiler/GHC/Utils/Outputable.hs#L1429-L1453
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?:Ok|Failed), (?:no|one|two|three|four|five|six|[0-9]+) modules? loaded.$")
            .unwrap()
    })
}
