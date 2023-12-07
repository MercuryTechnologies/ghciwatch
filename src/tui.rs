use crate::terminal;
use crate::ShutdownHandle;

/// TODO(evan): Document
pub async fn run_tui(shutdown: ShutdownHandle) -> miette::Result<()> {
    let _terminal = terminal::enter()?;

    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    let _ = shutdown.request_shutdown();

    Ok(())
}
