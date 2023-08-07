use std::sync::Weak;

use aho_corasick::AhoCorasick;
use tokio::io::Stdout;
use tokio::process::ChildStdout;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tracing::instrument;

use crate::aho_corasick::AhoCorasickExt;
use crate::ghci::PROMPT;
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
    pub buffer: Vec<u8>,
}

impl GhciStdout {
    #[instrument(skip_all, name = "stdout", level = "debug")]
    pub async fn run(mut self, when_ready: oneshot::Sender<()>) -> miette::Result<()> {
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
        self.prompt(&init_prompt_patterns, when_ready).await?;
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

        let prompt_patterns = AhoCorasick::from_anchored_patterns([PROMPT]);

        while let Some(event) = self.receiver.recv().await {
            match event {
                StdoutEvent::Sync(sentinel) => {
                    self.sync(&prompt_patterns, sentinel).await?;
                }
                StdoutEvent::Prompt(sender) => {
                    self.prompt(&prompt_patterns, sender).await?;
                }
                StdoutEvent::ShowModules(sender) => {
                    self.show_modules(&prompt_patterns, sender).await?;
                }
            }
        }

        Ok(())
    }

    #[instrument(skip_all, level = "debug")]
    async fn prompt(
        &mut self,
        prompt_patterns: &AhoCorasick,
        sender: oneshot::Sender<()>,
    ) -> miette::Result<()> {
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
    async fn sync(
        &mut self,
        prompt_patterns: &AhoCorasick,
        sentinel: SyncSentinel,
    ) -> miette::Result<()> {
        // Read until the sync marker...
        let sync_pattern = AhoCorasick::from_anchored_patterns([sentinel.to_string()]);
        let lines = self
            .reader
            .read_until(&sync_pattern, WriteBehavior::NoFinalLine, &mut self.buffer)
            .await?;
        // Then make sure to consume the prompt on the next line, and then we'll be caught up.
        let _ = self
            .reader
            .read_until(prompt_patterns, WriteBehavior::Hide, &mut self.buffer)
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
    async fn show_modules(
        &mut self,
        prompt_patterns: &AhoCorasick,
        sender: oneshot::Sender<ModuleSet>,
    ) -> miette::Result<()> {
        let lines = self
            .reader
            .read_until(prompt_patterns, WriteBehavior::Hide, &mut self.buffer)
            .await?;
        let map = ModuleSet::from_lines(&lines)?;
        let _ = sender.send(map);
        Ok(())
    }
}
