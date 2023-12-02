use crate::{ghci::manager::GhciEvent, terminal, ShutdownHandle};
use miette::WrapErr as _;
use tokio::sync::mpsc;

/// TODO(evan): Document
pub async fn run_tui(
    _handle: ShutdownHandle,
    _ghci_sender: mpsc::Sender<GhciEvent>,
) -> miette::Result<()> {
    let mut _terminal = terminal::enter().wrap_err("Failed to enter terminal")?;

    Ok(())
}
