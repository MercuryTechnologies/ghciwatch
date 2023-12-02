use crate::{ghci::manager::GhciEvent, ShutdownHandle};
use tokio::sync::mpsc;

/// TODO(evan): Document
pub async fn run_tui(
    _handle: ShutdownHandle,
    _ghci_sender: mpsc::Sender<GhciEvent>,
) -> miette::Result<()> {
    Ok(())
}
