//! Subsystem for [`Ghci`] to support graceful shutdown.

use std::collections::BTreeSet;
use std::process::ExitStatus;
use std::sync::Arc;

use miette::Context;
use miette::IntoDiagnostic;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tracing::instrument;

use crate::event_filter::FileEvent;
use crate::ghci::CompilationLog;
use crate::haskell_source_file::is_haskell_source_file;
use crate::hooks;
use crate::hooks::LifecycleEvent;
use crate::shutdown::ShutdownHandle;

use super::FileClassifier;
use super::Ghci;
use super::GhciOpts;
use super::GhciReloadKind;
use super::ModuleSet;

/// An event sent to [`Ghci`] by the watcher.
#[derive(Debug, Clone)]
pub enum WatcherEvent {
    /// Reload the `ghci` session.
    Reload {
        /// The file events to respond to.
        events: BTreeSet<FileEvent>,
    },
}

impl WatcherEvent {
    /// When we interrupt an event to reload, add the file events together so that we don't lose
    /// work.
    fn merge(&mut self, other: WatcherEvent) {
        match (self, other) {
            (
                WatcherEvent::Reload { events },
                WatcherEvent::Reload {
                    events: other_events,
                },
            ) => {
                events.extend(other_events);
            }
        }
    }
}

/// Start the [`Ghci`] subsystem.
#[instrument(skip_all, level = "debug")]
pub async fn run_ghci(
    mut handle: ShutdownHandle,
    opts: GhciOpts,
    mut receiver: mpsc::Receiver<WatcherEvent>,
) -> miette::Result<()> {
    // This function is pretty tricky! We need to handle shutdowns at each stage, and the process
    // is a little different each time, so the `select!`s can't be consolidated.

    let interrupt_reloads = opts.interrupt_reloads;
    let classifier = opts.file_classifier()?;
    let (exited_sender, mut exited_receiver) = mpsc::channel::<ExitStatus>(1);
    let mut ghci = Ghci::new(handle.clone(), opts, exited_sender)
        .await
        .wrap_err("Failed to start `ghci`")?;

    // Wait for ghci to finish loading.
    let mut log = CompilationLog::default();
    // Use biased select with exited_receiver before startup_result. When ghci dies, its stdout
    // closes and read_until enters a yield loop (returning Ok(None) on each EOF read), so
    // startup_result never completes. exited_receiver.recv() fires once GhciProcess detects the
    // exit, and with biased polling it is guaranteed to win once a message is available.
    let startup_exit: Option<ExitStatus> = tokio::select! {
        biased;
        _ = handle.on_shutdown_requested() => {
            ghci.stop().await.wrap_err("Failed to quit ghci")?;
            return Ok(());
        }
        Some(status) = exited_receiver.recv() => Some(status),
        startup_result = ghci.initialize(&mut log, [LifecycleEvent::Startup(hooks::When::After)]) => {
            // Only reachable if ghci starts successfully (startup_result = Ok) or if
            // initialization fails for a non-EOF reason (e.g., a lifecycle hook error).
            startup_result?;
            None
        }
    };
    if let Some(status) = startup_exit {
        match wait_and_restart(
            &mut handle,
            &mut receiver,
            &mut exited_receiver,
            &classifier,
            status,
            &mut RestartStrategy::Startup(&mut ghci),
        )
        .await?
        {
            RetryResult::Restarted => {}
            RetryResult::Shutdown => return Ok(()),
        }
    }

    let manager = GhciManager {
        ghci: Arc::new(Mutex::new(ghci)),
        handle,
        receiver,
        exited_receiver,
        classifier,
        interrupt_reloads,
    };
    manager.run().await
}

#[instrument(level = "debug", skip(ghci, reload_sender))]
async fn dispatch(
    ghci: Arc<Mutex<Ghci>>,
    event: WatcherEvent,
    reload_sender: oneshot::Sender<GhciReloadKind>,
) -> miette::Result<()> {
    match event {
        WatcherEvent::Reload { events } => {
            ghci.lock().await.reload(events, reload_sender).await?;
        }
    }
    Ok(())
}

/// Should we interrupt a reload with a new event?
#[instrument(level = "debug", skip_all)]
async fn should_interrupt(reload_receiver: oneshot::Receiver<GhciReloadKind>) -> bool {
    let reload_kind = match reload_receiver.await {
        Ok(kind) => kind,
        Err(err) => {
            tracing::debug!("Failed to receive reload kind from ghci: {err}");
            return false;
        }
    };

    match reload_kind {
        GhciReloadKind::None | GhciReloadKind::Restart => {
            // Nothing to do, wait for the task to finish.
            tracing::debug!(?reload_kind, "Not interrupting reload");
            false
        }
        GhciReloadKind::Reload => {
            tracing::debug!(?reload_kind, "Interrupting reload");
            true
        }
    }
}

/// Manages the main event loop for a running ghci session.
struct GhciManager {
    ghci: Arc<Mutex<Ghci>>,
    handle: ShutdownHandle,
    receiver: mpsc::Receiver<WatcherEvent>,
    exited_receiver: mpsc::Receiver<ExitStatus>,
    classifier: FileClassifier,
    interrupt_reloads: bool,
}

/// Result of [`GhciManager::wait_for_event`].
enum WaitResult {
    /// A watcher event was received.
    Event(WatcherEvent),
    /// A shutdown was requested (or the watcher channel closed).
    Shutdown,
    /// ghci died and was successfully restarted; caller should continue the loop.
    Restarted,
}

/// Result of [`GhciManager::handle_event`].
enum HandleResult {
    /// The event was dispatched (or ghci died during dispatch but was restarted).
    Done,
    /// The reload was interrupted; the merged event should be retried next iteration.
    Interrupted(WatcherEvent),
    /// A shutdown was requested.
    Shutdown,
}

impl GhciManager {
    async fn run(mut self) -> miette::Result<()> {
        let mut maybe_event: Option<WatcherEvent> = None;
        loop {
            let event = match maybe_event.take() {
                Some(event) => event,
                None => match self.wait_for_event().await? {
                    WaitResult::Event(event) => event,
                    WaitResult::Shutdown => break,
                    WaitResult::Restarted => continue,
                },
            };
            match self.handle_event(event).await? {
                HandleResult::Done => {}
                HandleResult::Interrupted(event) => maybe_event = Some(event),
                HandleResult::Shutdown => break,
            }
        }

        Ok(())
    }

    /// Wait for the next watcher event, handling shutdown and ghci death along the way.
    async fn wait_for_event(&mut self) -> miette::Result<WaitResult> {
        let ghci_exited = {
            let GhciManager {
                ref ghci,
                ref mut handle,
                ref mut receiver,
                ref mut exited_receiver,
                ..
            } = *self;
            tokio::select! {
                _ = handle.on_shutdown_requested() => {
                    ghci.lock().await.stop().await
                        .wrap_err("Failed to quit ghci")?;
                    return Ok(WaitResult::Shutdown);
                }
                ret = receiver.recv() => {
                    match ret {
                        Some(event) => {
                            tracing::debug!(?event, "Received ghci event from watcher");
                            return Ok(WaitResult::Event(event));
                        }
                        None => {
                            // Channel closed — shutdown in progress.
                            tracing::debug!(
                                "Watcher event channel closed; shutting down"
                            );
                            ghci.lock().await.stop().await
                                .wrap_err("Failed to quit ghci")?;
                            return Ok(WaitResult::Shutdown);
                        }
                    }
                }
                Some(status) = exited_receiver.recv() => status,
            }
        };
        // self is no longer partially borrowed, so we can call methods.
        match self.wait_and_restart_runtime(ghci_exited).await? {
            RetryResult::Restarted => Ok(WaitResult::Restarted),
            RetryResult::Shutdown => Ok(WaitResult::Shutdown),
        }
    }

    /// Dispatch a watcher event, handling shutdown, interruption, and ghci death.
    async fn handle_event(&mut self, mut event: WatcherEvent) -> miette::Result<HandleResult> {
        let (reload_sender, reload_receiver) = oneshot::channel();
        let mut task = Box::pin(tokio::task::spawn(dispatch(
            self.ghci.clone(),
            event.clone(),
            reload_sender,
        )));

        let ghci_exited = {
            let GhciManager {
                ref ghci,
                ref mut handle,
                ref mut receiver,
                ref mut exited_receiver,
                interrupt_reloads,
                ..
            } = *self;
            tokio::select! {
                _ = handle.on_shutdown_requested() => {
                    // Cancel any in-progress reloads. This releases the lock so we don't
                    // block here.
                    task.abort();
                    ghci.lock().await.stop().await
                        .wrap_err("Failed to quit ghci")?;
                    return Ok(HandleResult::Shutdown);
                }
                Some(status) = exited_receiver.recv() => {
                    // ghci died during the dispatch. Abort the stuck task to release the
                    // Mutex.
                    task.abort();
                    Some(status)
                }
                Some(new_event) = receiver.recv() => {
                    tracing::debug!(
                        ?new_event,
                        "Received ghci event from watcher while reloading"
                    );
                    if interrupt_reloads
                        && should_interrupt(reload_receiver).await
                    {
                        // Merge the events together so we don't lose progress.
                        event.merge(new_event);

                        // Cancel the in-progress reload. This releases the `ghci` lock to
                        // prevent a deadlock.
                        task.abort();

                        // Send a SIGINT to interrupt the reload.
                        // NB: This may take a couple seconds to register.
                        ghci.lock().await.send_sigint().await?;

                        return Ok(HandleResult::Interrupted(event));
                    }
                    None
                }
                ret = &mut task => {
                    ret.into_diagnostic()??;
                    tracing::debug!("Finished dispatching ghci event");
                    None
                }
            }
        };

        // If ghci died during the dispatch, wait for a file change and restart.
        if let Some(status) = ghci_exited {
            match self.wait_and_restart_runtime(status).await? {
                RetryResult::Restarted => {}
                RetryResult::Shutdown => return Ok(HandleResult::Shutdown),
            }
        }
        Ok(HandleResult::Done)
    }

    /// Wait for a relevant file change, then attempt to restart ghci.
    #[instrument(level = "debug", skip_all)]
    async fn wait_and_restart_runtime(
        &mut self,
        status: ExitStatus,
    ) -> miette::Result<RetryResult> {
        wait_and_restart(
            &mut self.handle,
            &mut self.receiver,
            &mut self.exited_receiver,
            &self.classifier,
            status,
            &mut RestartStrategy::Runtime(self.ghci.clone()),
        )
        .await
    }
}

/// Drain all pending events from the receiver and merge them into `event`.
fn drain_pending(event: &mut WatcherEvent, receiver: &mut mpsc::Receiver<WatcherEvent>) {
    while let Ok(new_event) = receiver.try_recv() {
        event.merge(new_event);
    }
}

/// Check whether an event would trigger a reload or restart.
///
/// Uses a default (empty) module set for classification, which correctly
/// identifies restart, reload, and add actions. The one gap: remove-module
/// actions require knowing the loaded targets, so we conservatively treat any
/// `Remove` of a Haskell source file as relevant. This may produce a false
/// positive (e.g. for files in the reload-ignore list), but a needless dispatch
/// is harmless — the real classify inside `reload()` will filter it out.
#[expect(unused)]
fn is_relevant(event: &WatcherEvent, classifier: &FileClassifier) -> miette::Result<bool> {
    let WatcherEvent::Reload { ref events } = *event;
    let kind = classifier
        .classify(events.clone(), &ModuleSet::default())?
        .kind();
    if !matches!(kind, GhciReloadKind::None) {
        return Ok(true);
    }
    // classify with an empty module set misses remove-module actions because
    // targets.contains_source_path is always false. Conservatively treat any
    // removed Haskell source file as relevant.
    Ok(events
        .iter()
        .any(|e| matches!(e, FileEvent::Remove(_)) && is_haskell_source_file(e.as_path())))
}

/// Drain all pending events from the receiver, merge them, classify, and return the kind.
/// Returns `None` when the combined events are irrelevant ([`GhciReloadKind::None`]).
fn drain_and_classify(
    initial: WatcherEvent,
    receiver: &mut mpsc::Receiver<WatcherEvent>,
    classifier: &FileClassifier,
) -> miette::Result<Option<GhciReloadKind>> {
    let mut event = initial;
    drain_pending(&mut event, receiver);
    let WatcherEvent::Reload { events } = event;
    let kind = classifier.classify(events, &ModuleSet::default())?.kind();
    if matches!(kind, GhciReloadKind::None) {
        Ok(None)
    } else {
        Ok(Some(kind))
    }
}

/// Outcome of [`wait_and_restart`].
enum RetryResult {
    /// ghci was successfully restarted.
    Restarted,
    /// A shutdown was requested while waiting.
    Shutdown,
}

/// How to restart ghci — differs between initial startup and runtime.
enum RestartStrategy<'a> {
    /// ghci failed during first startup; use [`Ghci::startup_restart`].
    Startup(&'a mut Ghci),
    /// ghci died at runtime; lock the [`Arc`] and call [`Ghci::startup_restart`].
    Runtime(Arc<Mutex<Ghci>>),
}

impl RestartStrategy<'_> {
    fn context(&self) -> &'static str {
        match self {
            Self::Startup(_) => "during startup",
            Self::Runtime(_) => "unexpectedly",
        }
    }

    async fn restart(&mut self) -> miette::Result<()> {
        match self {
            Self::Startup(ghci) => ghci
                .startup_restart()
                .await
                .wrap_err("Failed to restart ghci after startup failure"),
            Self::Runtime(ghci) => ghci
                .lock()
                .await
                .startup_restart()
                .await
                .wrap_err("Failed to restart ghci after unexpected exit"),
        }
    }
}

/// Wait for a relevant file change, then attempt to restart ghci.
///
/// If ghci also dies during the restart attempt, keeps waiting for file changes and retrying
/// rather than crashing. Returns [`RetryResult::Shutdown`] if a shutdown is requested while
/// waiting.
async fn wait_and_restart(
    handle: &mut ShutdownHandle,
    receiver: &mut mpsc::Receiver<WatcherEvent>,
    exited_receiver: &mut mpsc::Receiver<ExitStatus>,
    classifier: &FileClassifier,
    mut status: ExitStatus,
    strategy: &mut RestartStrategy<'_>,
) -> miette::Result<RetryResult> {
    let context = strategy.context();
    tracing::warn!(
        %status,
        "ghci exited {context}; waiting for a file change to restart",
    );
    loop {
        // Wait for a watcher event to use as a restart trigger. We handle both the shutdown
        // signal and the channel closing (which also indicates shutdown, since the sender is
        // exclusively owned by `run_watcher` and it only exits on shutdown).
        tokio::select! {
            _ = handle.on_shutdown_requested() => {
                // ghci is already dead; nothing to stop.
                return Ok(RetryResult::Shutdown);
            }
            ret = receiver.recv() => {
                let Some(event) = ret else {
                    // Channel closed — shutdown in progress. ghci is already dead.
                    tracing::debug!("Watcher event channel closed; shutting down");
                    return Ok(RetryResult::Shutdown);
                };
                if drain_and_classify(event, receiver, classifier)?.is_none() {
                    tracing::debug!("File change not relevant to ghci; continuing to wait");
                    continue;
                }
            }
        }
        tracing::debug!("Restarting ghci");
        // Race restart against exited_receiver. When ghci dies during restart's
        // initialize(), read_until loops with yields (rather than erroring on EOF), so
        // restart never completes on its own. exited_receiver fires when GhciProcess
        // detects the exit, and with biased polling it wins first so we can retry.
        tokio::select! {
            biased;
            Some(new_status) = exited_receiver.recv() => {
                status = new_status;
                tracing::warn!(
                    %status,
                    "ghci exited {context}; waiting for a file change to restart",
                );
            }
            result = strategy.restart() => {
                result?;
                return Ok(RetryResult::Restarted);
            }
        }
    }
}
