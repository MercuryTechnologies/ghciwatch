//! Subsystem for [`Ghci`] to support graceful shutdown.

use std::collections::BTreeSet;

use miette::miette;
use miette::Context;
use tokio::sync::mpsc;
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

    loop {
        let recv_and_dispatch = async {
            let event = receiver
                .recv()
                .await
                .ok_or_else(|| miette!("ghci event channel closed"))?;

            tracing::debug!(?event, "Received ghci event from watcher");
            dispatch(&mut ghci, event).await?;
            tracing::debug!("Finished dispatching ghci event");

            Ok(()) as miette::Result<()>
        };

        tokio::select! {
            _ = handle.on_shutdown_requested() => {
                tracing::debug!("shutdown requested in ghci manager");
                ghci.stop().await.wrap_err("Failed to quit ghci")?;
                break;
            }
            ret = recv_and_dispatch => {
                ret?;
            }
        }
    }

    Ok(())
}

#[instrument(level = "debug", skip(ghci))]
async fn dispatch(ghci: &mut Ghci, event: GhciEvent) -> miette::Result<()> {
    match event {
        GhciEvent::Reload { events } => {
            ghci.reload(events).await?;
        }
    }
    Ok(())
}
