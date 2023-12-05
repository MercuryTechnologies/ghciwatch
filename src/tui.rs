use crate::{ghci::manager::GhciEvent, terminal, ShutdownHandle};
use crossterm::event::{Event, EventStream, KeyCode, KeyModifiers};
use miette::{miette, IntoDiagnostic as _, WrapErr as _};
use ratatui::style::Style;
use std::str;
use tokio::{
    io::{AsyncReadExt as _, DuplexStream},
    sync::mpsc,
};
use tokio_stream::StreamExt as _;

/// TODO(evan): Document
pub async fn run_tui(
    shutdown_handle: ShutdownHandle,
    mut tui_reader: DuplexStream,
    _ghci_sender: mpsc::Sender<GhciEvent>,
) -> miette::Result<()> {
    let mut scrollback = String::new();

    let mut buffer = [0; 1024];

    let mut terminal = terminal::enter().wrap_err("Failed to enter terminal")?;

    let mut event_stream = EventStream::new();

    loop {
        tui_reader
            .read(&mut buffer)
            .await
            .into_diagnostic()
            .wrap_err("Failed to read bytes into TUI buffer")?;

        // TODO(evan): It's not always valid UTF-8!!
        let str = str::from_utf8(&buffer[..])
            .into_diagnostic()
            .wrap_err("Bytes are not valid UTF-8")?;

        scrollback.push_str(str);

        terminal
            .draw(|frame| {
                let area = frame.size();
                let buffer = frame.buffer_mut();
                // TODO(evan): Scroll once you hit the bottom
                for (i, line) in scrollback.lines().enumerate() {
                    if i < usize::from(area.bottom()) {
                        let y = u16::try_from(i).unwrap();
                        buffer.set_string(area.x, y, line, Style::new());
                    }
                }
            })
            .into_diagnostic()
            .wrap_err("Failed to draw to terminal")?;

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

    shutdown_handle
        .request_shutdown()
        .into_diagnostic()
        .wrap_err("Failed to request shutdown")?;

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
