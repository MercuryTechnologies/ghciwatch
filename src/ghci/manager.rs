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

    let no_interrupt_reloads = opts.no_interrupt_reloads;
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
    if let Some(mut status) = startup_exit {
        tracing::warn!(
            %status,
            "ghci exited during startup; waiting for a file change to restart"
        );
        loop {
            tokio::select! {
                _ = handle.on_shutdown_requested() => {
                    tracing::debug!("ghci is already dead; nothing to stop");
                    return Ok(());
                }
                ret = receiver.recv() => {
                    let Some(mut event) = ret else {
                        // Channel closed — `run_watcher` exited, which only happens
                        // during shutdown. Treat as clean shutdown.
                        tracing::debug!("Watcher event channel closed; shutting down");
                        return Ok(());
                    };
                    // Merge as many events as possible from the queue.
                    while let Ok(new_event) = receiver.try_recv() {
                        event.merge(new_event);
                    }
                    let WatcherEvent::Reload { events } = event;
                    let actions = classifier.classify(events, &ModuleSet::default())?;
                    if matches!(actions.kind(), GhciReloadKind::None) {
                        tracing::debug!("File change not relevant to ghci; continuing to wait");
                        continue;
                    }
                }
            }
            tracing::debug!("Restarting ghci");
            // Race restart() against exited_receiver. When ghci dies during restart's
            // initialize(), read_until loops with yields rather than erroring (to avoid a
            // race with exited_receiver). So restart() will never return on its own when
            // ghci dies — exited_receiver fires first, and with biased polling it wins.
            tokio::select! {
                biased;
                Some(new_status) = exited_receiver.recv() => {
                    status = new_status;
                    tracing::warn!(
                        %status,
                        "ghci exited during startup; waiting for a file change to restart"
                    );
                }
                result = ghci.startup_restart() => {
                    result.wrap_err("Failed to restart ghci after startup failure")?;
                    break;
                }
            }
        }
    }

    let ghci = Arc::new(Mutex::new(ghci));
    // The event to respond to. If we interrupt a reload, we may begin the loop with `Some(_)` in
    // here.
    let mut maybe_event = None;
    'main: loop {
        let mut event = match maybe_event.take() {
            Some(event) => event,
            None => {
                // If we don't already have an event to respond to, wait for filesystem events.
                let event = tokio::select! {
                    _ = handle.on_shutdown_requested() => {
                        ghci.lock().await.stop().await.wrap_err("Failed to quit ghci")?;
                        break;
                    }
                    ret = receiver.recv() => {
                        match ret {
                            Some(event) => event,
                            None => {
                                // Channel closed — shutdown in progress.
                                tracing::debug!("Watcher event channel closed; shutting down");
                                ghci.lock().await.stop().await.wrap_err("Failed to quit ghci")?;
                                break;
                            }
                        }
                    }
                    Some(status) = exited_receiver.recv() => {
                        match wait_and_restart(&ghci, &mut handle, &mut receiver, &mut exited_receiver, &classifier, status).await? {
                            RetryResult::Restarted => {},
                            RetryResult::Shutdown => break 'main,
                        }
                        continue 'main;
                    }
                };
                tracing::debug!(?event, "Received ghci event from watcher");
                event
            }
        };

        // This channel notifies us what kind of reload is triggered, which we can use to inform
        // our decision to interrupt the reload or not.
        let (reload_sender, reload_receiver) = oneshot::channel();
        // Dispatch the event. We spawn it into a new task so it can run in parallel to any
        // shutdown requests.
        let mut task = Box::pin(tokio::task::spawn(dispatch(
            ghci.clone(),
            event.clone(),
            reload_sender,
        )));
        let mut dispatch_exit = None;
        tokio::select! {
            _ = handle.on_shutdown_requested() => {
                // Cancel any in-progress reloads. This releases the lock so we don't block here.
                task.abort();
                ghci.lock().await.stop().await.wrap_err("Failed to quit ghci")?;
                break;
            }
            Some(status) = exited_receiver.recv() => {
                // ghci died during the dispatch. Abort the stuck task to release the Mutex.
                task.abort();
                dispatch_exit = Some(status);
            }
            Some(new_event) = receiver.recv() => {
                tracing::debug!(?new_event, "Received ghci event from watcher while reloading");
                if !no_interrupt_reloads && should_interrupt(reload_receiver).await {
                    // Merge the events together so we don't lose progress.
                    // Then, the next iteration of the loop will pick up the `maybe_event` value
                    // and respond immediately.
                    event.merge(new_event);
                    maybe_event = Some(event);

                    // Cancel the in-progress reload. This releases the `ghci` lock to prevent a deadlock.
                    task.abort();

                    {
                        let mut ghci_guard = ghci.lock().await;

                        // Send a SIGINT to interrupt the reload.
                        // NB: This may take a couple seconds to register.
                        ghci_guard.send_sigint().await?;

                        // The abort may have interrupted `reload()` between a GHCi
                        // command (`:add`/`:unadd`) and the corresponding update to
                        // `self.targets` or `self.eval_commands`, leaving in-memory
                        // state out of sync with GHCi. Re-sync from ground truth.
                        ghci_guard.refresh_targets().await?;
                        ghci_guard.refresh_eval_commands().await?;
                        ghci_guard.prune_command_handles();
                    }
                }
            }
            ret = &mut task => {
                ret.into_diagnostic()??;
                tracing::debug!("Finished dispatching ghci event");
            }
        }

        // If ghci died during the dispatch, wait for a file change and restart.
        if let Some(status) = dispatch_exit {
            match wait_and_restart(
                &ghci,
                &mut handle,
                &mut receiver,
                &mut exited_receiver,
                &classifier,
                status,
            )
            .await?
            {
                RetryResult::Restarted => {}
                RetryResult::Shutdown => break,
            }
        }
    }

    Ok(())
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

/// Outcome of [`wait_and_restart`].
enum RetryResult {
    /// ghci was successfully restarted.
    Restarted,
    /// A shutdown was requested while waiting.
    Shutdown,
}

/// Wait for a relevant file change, then attempt to restart ghci.
///
/// If ghci also dies during the restart attempt, keeps waiting for file changes and retrying
/// rather than crashing. Returns [`RetryResult::Shutdown`] if a shutdown is requested while
/// waiting.
#[instrument(level = "debug", skip_all)]
async fn wait_and_restart(
    ghci: &Arc<Mutex<Ghci>>,
    handle: &mut ShutdownHandle,
    receiver: &mut mpsc::Receiver<WatcherEvent>,
    exited_receiver: &mut mpsc::Receiver<ExitStatus>,
    classifier: &FileClassifier,
    mut status: ExitStatus,
) -> miette::Result<RetryResult> {
    tracing::warn!(
        %status,
        "ghci exited unexpectedly; waiting for a file change to restart"
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
                let Some(mut event) = ret else {
                    // Channel closed — shutdown in progress. ghci is already dead.
                    tracing::debug!("Watcher event channel closed; shutting down");
                    return Ok(RetryResult::Shutdown);
                };
                // Merge as many events as possible from the queue.
                while let Ok(new_event) = receiver.try_recv() {
                    event.merge(new_event);
                }
                let WatcherEvent::Reload { events } = event;
                let actions = classifier.classify(events, &ModuleSet::default())?;
                if matches!(actions.kind(), GhciReloadKind::None) {
                    tracing::debug!("File change not relevant to ghci; continuing to wait");
                    continue;
                }
            }
        }
        // Race restart() against exited_receiver. When ghci dies during restart's
        // initialize(), read_until loops with yields (rather than erroring on EOF), so
        // restart() never completes on its own. exited_receiver fires when GhciProcess
        // detects the exit, and with biased polling it wins first so we can retry.
        tokio::select! {
            biased;
            Some(new_status) = exited_receiver.recv() => {
                status = new_status;
                tracing::warn!(
                    %status,
                    "ghci exited unexpectedly; waiting for a file change to restart"
                );
            }
            result = async { ghci.lock().await.restart().await } => {
                result.wrap_err("Failed to restart ghci after unexpected exit")?;
                return Ok(RetryResult::Restarted);
            }
        }
    }
}
