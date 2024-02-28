mod terminal;

use crate::ShutdownHandle;
use ansi_to_tui::IntoText;
use crossterm::event::Event;
use crossterm::event::EventStream;
use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEventKind;
use miette::miette;
use miette::IntoDiagnostic;
use miette::WrapErr;
use ratatui::prelude::Buffer;
use ratatui::prelude::Rect;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;
use std::cmp::min;
use tokio::io::AsyncReadExt;
use tokio::io::DuplexStream;
use tokio_stream::StreamExt;

/// State data for drawing the TUI.
#[derive(Default)]
struct Tui {
    quit: bool,
    scrollback: Vec<u8>,
    // TODO(evan): Follow output when scrolled to bottom
    scroll_offset: usize,
}

/// Start the terminal event loop, reading output from the given readers.
pub async fn run_tui(
    mut shutdown: ShutdownHandle,
    mut ghci_reader: DuplexStream,
    mut tracing_reader: DuplexStream,
) -> miette::Result<()> {
    let mut ghci_buffer = [0; crate::buffers::GHCI_BUFFER_CAPACITY];
    let mut tracing_buffer = [0; crate::buffers::TRACING_BUFFER_CAPACITY];

    let mut terminal = terminal::enter()?;

    let mut tui = Tui::default();

    let mut event_stream = EventStream::new();

    while !tui.quit {
        let mut render_result = Ok(());
        terminal
            .draw(|frame| {
                let area = frame.size();
                let buffer = frame.buffer_mut();
                render_result = render(&tui, area, buffer);
            })
            .into_diagnostic()
            .wrap_err("Failed to draw to terminal")?;
        render_result?;

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
                    tui.scrollback.extend(ghci_buffer);
                    ghci_buffer = [0; 1024];
                }
            }

            output = tracing_reader.read(&mut tracing_buffer) => {
                output
                    .into_diagnostic()
                    .wrap_err("Failed to read bytes from tracing into TUI buffer")?;
                tui.scrollback.extend(tracing_buffer);
                tracing_buffer = [0; 1024];
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

fn render(tui: &Tui, area: Rect, buffer: &mut Buffer) -> miette::Result<()> {
    if area.width == 0 || area.height == 0 {
        return Ok(());
    }

    let text = tui.scrollback.into_text().into_diagnostic()?;

    let scroll_offset = u16::try_from(tui.scroll_offset).unwrap();

    Paragraph::new(text)
        .wrap(Wrap::default())
        .scroll((scroll_offset, 0))
        .render(area, buffer);

    Ok(())
}

const SCROLL_AMOUNT: usize = 1;

fn handle_event(tui: &mut Tui, event: Event) -> miette::Result<()> {
    match event {
        // TODO(evan): Scrolling is excruciatingly slow
        Event::Mouse(mouse) if mouse.kind == MouseEventKind::ScrollUp => {
            tui.scroll_offset = tui.scroll_offset.saturating_sub(SCROLL_AMOUNT);
        }
        Event::Mouse(mouse) if mouse.kind == MouseEventKind::ScrollDown => {
            let last_line = tui
                .scrollback
                .split(|byte| *byte == b'\n')
                .count()
                .saturating_sub(1);
            tui.scroll_offset = min(last_line, tui.scroll_offset + SCROLL_AMOUNT);
        }
        Event::Key(key)
            if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') =>
        {
            tui.quit = true;
        }
        _ => {}
    }

    Ok(())
}
