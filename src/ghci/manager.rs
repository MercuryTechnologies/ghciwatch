//! Subsystem for [`Ghci`] to support graceful shutdown.

use std::collections::BTreeSet;
use std::sync::Arc;

use miette::miette;
use miette::Context;
use miette::IntoDiagnostic;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tracing::instrument;

use crate::event_filter::FileEvent;
use crate::shutdown::ShutdownHandle;

use super::Ghci;
use super::GhciOpts;

/// An event sent to [`Ghci`].
#[derive(Debug)]
pub enum GhciEvent {
    /// Reload the `ghci` session.
    Reload {
        /// The file events to respond to.
        events: BTreeSet<FileEvent>,
    },
}

/// Start the [`Ghci`] subsystem.
#[instrument(skip_all, level = "debug")]
pub async fn run_ghci(
    mut handle: ShutdownHandle,
    opts: GhciOpts,
    mut receiver: mpsc::Receiver<GhciEvent>,
) -> miette::Result<()> {
    let mut ghci = Ghci::new(handle.clone(), opts)
        .await
        .wrap_err("Failed to start `ghci`")?;

    tokio::select! {
        _ = handle.on_shutdown_requested() => {
            tracing::debug!("shutdown requested in ghci manager");
            ghci.stop().await.wrap_err("Failed to quit ghci")?;
        }
        startup_result = ghci.initialize() => {
            startup_result?;
        }
    }

    let ghci = Arc::new(Mutex::new(ghci));

    loop {
        let recv_ghci = ghci.clone();

        let recv = async {
            receiver
                .recv()
                .await
                .ok_or_else(|| miette!("ghci event channel closed"))
        };

        let event = tokio::select! {
            _ = handle.on_shutdown_requested() => {
                tracing::debug!("shutdown requested in ghci manager");
                ghci.lock().await.stop().await.wrap_err("Failed to quit ghci")?;
                break;
            }
            ret = recv => {
                ret?
            }
        };
        tracing::debug!(?event, "Received ghci event from watcher");

        let mut task = Box::pin(tokio::task::spawn(dispatch(recv_ghci, event)));

        tokio::select! {
            _ = handle.on_shutdown_requested() => {
                tracing::debug!("shutdown requested in ghci manager");
                task.abort();
                ghci.lock().await.stop().await.wrap_err("Failed to quit ghci")?;
                break;
            }
            ret = &mut task => {
                ret.into_diagnostic()??;
                tracing::debug!("Finished dispatching ghci event");
            }
        }
    }

    Ok(())
}

#[instrument(level = "debug", skip(ghci))]
async fn dispatch(ghci: Arc<Mutex<Ghci>>, event: GhciEvent) -> miette::Result<()> {
    match event {
        GhciEvent::Reload { events } => {
            ghci.lock().await.reload(events).await?;
        }
    }
    Ok(())
}
