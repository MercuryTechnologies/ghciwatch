use crate::{ghci::manager::GhciEvent, terminal, ShutdownHandle};
use async_dup::{Arc, Mutex};
use crossterm::event::{Event, EventStream, KeyCode, KeyModifiers};
use miette::{miette, IntoDiagnostic as _, WrapErr as _};
use ratatui::style::Style;
use std::str;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _, DuplexStream},
    sync::mpsc,
};
use tokio_stream::StreamExt as _;
use tokio_util::compat::Compat;

type ClonableDuplexStream = Compat<Arc<Mutex<Compat<DuplexStream>>>>;

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

        let str = str::from_utf8(&buffer[..])
            .into_diagnostic()
            .wrap_err("Bytes are not valid UTF-8")?;

        scrollback.push_str(str);

        terminal
            .draw(|frame| {
                let area = frame.size();
                let buffer = frame.buffer_mut();
                buffer.set_string(area.x, area.y, &scrollback, Style::new());
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

/// TODO(evan): Remove
pub async fn write_hello_world(mut tui_writer: ClonableDuplexStream) -> miette::Result<()> {
    tui_writer
        .write_all(b"Hello, world!")
        .await
        .into_diagnostic()
        .wrap_err("Failed to write 'hello world' text to TUI")
}
