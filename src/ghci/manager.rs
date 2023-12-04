//! Subsystem for [`Ghci`] to support graceful shutdown.

use std::collections::BTreeSet;
use std::sync::Arc;

use miette::miette;
use miette::Context;
use miette::IntoDiagnostic;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tracing::instrument;

use crate::event_filter::FileEvent;
use crate::ghci::CompilationLog;
use crate::shutdown::ShutdownHandle;

use super::Ghci;
use super::GhciOpts;
use super::GhciReloadKind;

/// An event sent to [`Ghci`].
#[derive(Debug, Clone)]
pub enum GhciEvent {
    /// Reload the `ghci` session.
    Reload {
        /// The file events to respond to.
        events: BTreeSet<FileEvent>,
    },
}

impl GhciEvent {
    /// When we interrupt an event to reload, add the file events together so that we don't lose
    /// work.
    fn merge(&mut self, other: GhciEvent) {
        match (self, other) {
            (
                GhciEvent::Reload { events },
                GhciEvent::Reload {
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
    mut receiver: mpsc::Receiver<GhciEvent>,
) -> miette::Result<()> {
    // This function is pretty tricky! We need to handle shutdowns at each stage, and the process
    // is a little different each time, so the `select!`s can't be consolidated.

    let mut ghci = Ghci::new(handle.clone(), opts)
        .await
        .wrap_err("Failed to start `ghci`")?;

    // Wait for ghci to finish loading.
    let mut log = CompilationLog::default();
    tokio::select! {
        _ = handle.on_shutdown_requested() => {
            ghci.stop().await.wrap_err("Failed to quit ghci")?;
        }
        startup_result = ghci.initialize(&mut log) => {
            startup_result?;
        }
    }

    let ghci = Arc::new(Mutex::new(ghci));
    // The event to respond to. If we interrupt a reload, we may begin the loop with `Some(_)` in
    // here.
    let mut maybe_event = None;
    loop {
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
                        ret.ok_or_else(|| miette!("ghci event channel closed"))?
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
        tokio::select! {
            _ = handle.on_shutdown_requested() => {
                // Cancel any in-progress reloads. This releases the lock so we don't block here.
                task.abort();
                ghci.lock().await.stop().await.wrap_err("Failed to quit ghci")?;
                break;
            }
            Some(new_event) = receiver.recv() => {
                tracing::debug!(?new_event, "Received ghci event from watcher while reloading");
                if should_interrupt(reload_receiver).await {
                    // Merge the events together so we don't lose progress.
                    // Then, the next iteration of the loop will pick up the `maybe_event` value
                    // and respond immediately.
                    event.merge(new_event);
                    maybe_event = Some(event);

                    // Cancel the in-progress reload. This releases the `ghci` lock to prevent a deadlock.
                    task.abort();

                    // Send a SIGINT to interrupt the reload.
                    // NB: This may take a couple seconds to register.
                    ghci.lock().await.send_sigint().await?;
                }
            }
            ret = &mut task => {
                ret.into_diagnostic()??;
                tracing::debug!("Finished dispatching ghci event");
            }
        }
    }

    Ok(())
}

#[instrument(level = "debug", skip(ghci, reload_sender))]
async fn dispatch(
    ghci: Arc<Mutex<Ghci>>,
    event: GhciEvent,
    reload_sender: oneshot::Sender<GhciReloadKind>,
) -> miette::Result<()> {
    match event {
        GhciEvent::Reload { events } => {
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
