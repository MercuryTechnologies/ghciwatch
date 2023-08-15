use std::sync::OnceLock;

use aho_corasick::AhoCorasick;
use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
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

/// An event sent to a `ghci` session's stdout channel.
#[derive(Debug)]
pub enum StdoutEvent {
    /// Wait for `ghci` startup indicators, then wait for the initial prompt.
    Initialize(oneshot::Sender<()>),
    /// Wait for a regular `ghci` prompt.
    Prompt(oneshot::Sender<Option<CompilationResult>>),
    /// Wait for a sync marker.
    Sync(SyncSentinel),
    /// Parse `:show modules` output.
    ShowModules(oneshot::Sender<ModuleSet>),
    /// Set the session's mode.
    Mode {
        mode: Mode,
        sender: oneshot::Sender<()>,
    },
}

pub struct GhciStdout {
    /// Reader for parsing and forwarding the underlying stdout stream.
    pub reader: IncrementalReader<ChildStdout, Stdout>,
    /// Channel for communicating with the stderr task.
    pub stderr_sender: mpsc::Sender<StderrEvent>,
    /// Channel for communicating with this task.
    pub receiver: mpsc::Receiver<StdoutEvent>,
    /// Prompt patterns to match. Constructing these `AhoCorasick` automatons is costly so we store
    /// them in the task state.
    pub prompt_patterns: AhoCorasick,
    /// A buffer to read data into. Lets us avoid allocating buffers in the [`IncrementalReader`].
    pub buffer: Vec<u8>,
    /// The mode we're currently reading output in.
    pub mode: Mode,
}

impl GhciStdout {
    #[instrument(skip_all, name = "stdout", level = "debug")]
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

    async fn run_inner(&mut self) -> miette::Result<()> {
        while let Some(event) = self.receiver.recv().await {
            match event {
                StdoutEvent::Initialize(sender) => {
                    self.initialize(sender).await?;
                }
                StdoutEvent::Sync(sentinel) => {
                    self.sync(sentinel).await?;
                }
                StdoutEvent::Prompt(sender) => {
                    self.prompt(sender, None).await?;
                }
                StdoutEvent::ShowModules(sender) => {
                    self.show_modules(sender).await?;
                }
                StdoutEvent::Mode { mode, sender } => {
                    self.set_mode(sender, mode).await;
                }
            }
        }

        Ok(())
    }

    #[instrument(skip_all, level = "debug")]
    async fn initialize(&mut self, when_ready: oneshot::Sender<()>) -> miette::Result<()> {
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
        let (sender, receiver) = oneshot::channel();
        self.prompt(sender, Some(&init_prompt_patterns)).await?;
        receiver.await.into_diagnostic()?;
        tracing::debug!("Saw initial `ghci> ` prompt");

        let _ = when_ready.send(());

        Ok(())
    }

    #[instrument(skip_all, level = "debug")]
    async fn prompt(
        &mut self,
        sender: oneshot::Sender<Option<CompilationResult>>,
        // We usually want this to be `&self.prompt_patterns`, but when we initialize we want to
        // pass in a different value. This method takes an `&mut self` reference, so if we try to
        // pass in `&self.prompt_patterns` when we call it we get a borrow error because the
        // compiler doesn't know we don't mess with `self.prompt_patterns` in here. So we use
        // `None` to represent that case and handle the default inline.
        prompt_patterns: Option<&AhoCorasick>,
    ) -> miette::Result<()> {
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

        let _ = sender.send(result);

        Ok(())
    }

    #[instrument(skip_all, level = "debug")]
    async fn sync(&mut self, sentinel: SyncSentinel) -> miette::Result<()> {
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
    async fn show_modules(&mut self, sender: oneshot::Sender<ModuleSet>) -> miette::Result<()> {
        let lines = self
            .reader
            .read_until(&self.prompt_patterns, WriteBehavior::Hide, &mut self.buffer)
            .await?;
        let map = ModuleSet::from_lines(&lines)?;
        let _ = sender.send(map);
        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    async fn set_mode(&mut self, sender: oneshot::Sender<()>, mode: Mode) {
        self.mode = mode;
        let _ = sender.send(());
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
                    CompilationResult::Ok
                } else {
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
            }
        }

        Ok(None)
    }
}

fn compilation_finished_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(?:Ok|Failed), [0-9]+ modules loaded.$").unwrap())
}
