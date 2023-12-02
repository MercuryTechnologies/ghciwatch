use crate::{ghci::manager::GhciEvent, terminal, ShutdownHandle};
use crossterm::event::{Event, EventStream, KeyCode, KeyModifiers};
use miette::{miette, IntoDiagnostic as _, WrapErr as _};
use tokio::sync::mpsc;
use tokio_stream::StreamExt as _;

/// TODO(evan): Document
pub async fn run_tui(
    _handle: ShutdownHandle,
    _ghci_sender: mpsc::Sender<GhciEvent>,
) -> miette::Result<()> {
    let mut _terminal = terminal::enter().wrap_err("Failed to enter terminal")?;

    let mut event_stream = EventStream::new();

    loop {
        let event = event_stream
            .next()
            .await
            .ok_or(miette!("No more crossterm events"))?
            .into_diagnostic()
            .wrap_err("Failed to get next crossterm event")?;

        let quit = handle_event(event);

        if quit {
            break;
        }
    }

    Ok(())
}

fn handle_event(event: Event) -> bool {
    let mut quit = false;

    match event {
        Event::Key(key)
            if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') =>
        {
            quit = true;
        }
        _ => {}
    }

    quit
}
