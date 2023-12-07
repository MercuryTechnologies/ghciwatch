mod terminal;

use crate::ShutdownHandle;
use crossterm::event::Event;
use crossterm::event::EventStream;
use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;
use miette::miette;
use miette::IntoDiagnostic;
use miette::WrapErr;
use ratatui::prelude::Buffer;
use ratatui::prelude::Rect;
use ratatui::prelude::Style;
use std::str;
use tokio::io::AsyncReadExt;
use tokio::io::DuplexStream;
use tokio_stream::StreamExt;

#[derive(Default)]
struct Tui {
    quit: bool,
    scrollback: String,
}

/// TODO(evan): Document
pub async fn run_tui(
    mut shutdown: ShutdownHandle,
    mut ghci_reader: DuplexStream,
) -> miette::Result<()> {
    let mut terminal = terminal::enter()?;

    let mut tui = Tui::default();

    let mut ghci_buffer = [0; 1024];

    let mut event_stream = EventStream::new();

    while !tui.quit {
        terminal
            .draw(|frame| {
                let area = frame.size();
                let buffer = frame.buffer_mut();
                render(&tui, area, buffer);
            })
            .into_diagnostic()
            .wrap_err("Failed to draw to terminal")?;

        tokio::select! {
            _ = shutdown.on_shutdown_requested() => {
                tui.quit = true;
            }

            output = ghci_reader.read(&mut ghci_buffer) => {
                let n = output
                    .into_diagnostic()
                    .wrap_err("Failed to read bytes from GHCi into TUI buffer")?;
                if n == 0 {
                    tui.quit = true;
                } else {
                    // TODO(evan): It's not always valid UTF-8!!
                    let str = str::from_utf8(&ghci_buffer[..])
                        .into_diagnostic()
                        .wrap_err("Bytes are not valid UTF-8")?;
                    tui.scrollback.push_str(str);
                    ghci_buffer = [0; 1024];
                }
            }

            output = event_stream.next() => {
                let event = output
                    .ok_or_else(|| miette!("No more crossterm events"))?
                    .into_diagnostic()
                    .wrap_err("Failed to get next crossterm event")?;
                handle_event(&mut tui, event)?;
            }
        }
    }

    let _ = shutdown.request_shutdown();

    Ok(())
}

// TODO(evan): Soft wrap lines
fn render(tui: &Tui, area: Rect, buffer: &mut Buffer) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let lines = tui.scrollback.lines().collect::<Vec<_>>();

    for y in area.top()..area.bottom() {
        let Some(line) = lines.get(usize::from(y)) else {
            break;
        };

        buffer.set_stringn(area.x, y, line, usize::from(area.width), Style::default());
    }
}

fn handle_event(tui: &mut Tui, event: Event) -> miette::Result<()> {
    match event {
        Event::Key(key)
            if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') =>
        {
            tui.quit = true;
        }
        _ => {}
    }

    Ok(())
}
