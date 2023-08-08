use std::sync::Weak;

use aho_corasick::AhoCorasick;
use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use tokio::io::Stdout;
use tokio::process::ChildStdout;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tracing::instrument;

use crate::aho_corasick::AhoCorasickExt;
use crate::incremental_reader::IncrementalReader;
use crate::incremental_reader::WriteBehavior;
use crate::sync_sentinel::SyncSentinel;

use super::show_modules::ModuleSet;
use super::stderr::StderrEvent;
use super::stdin::StdinEvent;
use super::Ghci;

/// An event sent to a `ghci` session's stdout channel.
#[derive(Debug)]
pub enum StdoutEvent {
    /// Wait for `ghci` startup indicators, then wait for the initial prompt.
    Initialize(oneshot::Sender<()>),
    /// Wait for a regular `ghci` prompt.
    Prompt(oneshot::Sender<()>),
    /// Wait for a sync marker.
    Sync(SyncSentinel),
    /// Parse `:show modules` output.
    ShowModules(oneshot::Sender<ModuleSet>),
}

pub struct GhciStdout {
    pub ghci: Weak<Mutex<Ghci>>,
    pub reader: IncrementalReader<ChildStdout, Stdout>,
    pub stdin_sender: mpsc::Sender<StdinEvent>,
    pub stderr_sender: mpsc::Sender<StderrEvent>,
    pub receiver: mpsc::Receiver<StdoutEvent>,
    pub prompt_patterns: AhoCorasick,
    pub buffer: Vec<u8>,
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
        tracing::debug!(?lines, "ghci started");

        let init_prompt_patterns = AhoCorasick::from_anchored_patterns(["ghci> "]);
        // We know that we'll get _one_ `ghci> ` prompt on startup.
        self.prompt(when_ready, Some(&init_prompt_patterns)).await?;
        // But we might get more than one `ghci> ` prompt, so digest any others from the buffer.
        while let Some(lines) = self
            .reader
            .try_read_until(
                &init_prompt_patterns,
                WriteBehavior::NoFinalLine,
                &mut self.buffer,
            )
            .await?
        {
            tracing::debug!(?lines, "initial ghci prompt");
        }

        Ok(())
    }

    #[instrument(skip_all, level = "debug")]
    async fn prompt(
        &mut self,
        sender: oneshot::Sender<()>,
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
        tracing::debug!(?lines, "Got data from ghci");
        // Tell the stderr stream to write the error log and then finish.
        let _ = self.stderr_sender.send(StderrEvent::Write(sender)).await;
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
}
