//! Subsystem for [`Ghci`] to support graceful shutdown.

use miette::Context;
use tokio::sync::mpsc;
use tracing::instrument;

use crate::event_filter::FileEvent;
use crate::ghci::process::GhciProcessState;
use crate::shutdown::ShutdownHandle;

use super::Ghci;
use super::GhciOpts;

/// An event sent to [`Ghci`].
#[derive(Debug)]
pub enum GhciEvent {
    /// Reload the `ghci` session.
    Reload {
        /// The file events to respond to.
        events: Vec<FileEvent>,
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

    loop {
        tokio::select! {
            _ = handle.on_shutdown_requested() => {
                if ghci.get_process_state().await == GhciProcessState::Running {
                    ghci.stop().await.wrap_err("Failed to quit ghci")?;
                }
                break;
            }
            Some(event) = receiver.recv() => {
                dispatch(&mut ghci, event).await?;
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
