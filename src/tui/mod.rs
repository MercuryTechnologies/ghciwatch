mod terminal;

use crate::async_buffer_redirect::AsyncBufferRedirect;
use crate::ShutdownHandle;
use ansi_to_tui::IntoText;
use crossterm::event::Event;
use crossterm::event::EventStream;
use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;
use miette::miette;
use miette::IntoDiagnostic;
use miette::WrapErr;
use ratatui::prelude::Buffer;
use ratatui::prelude::Rect;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;
use tokio::io::AsyncReadExt;
use tokio::io::DuplexStream;
use tokio_stream::StreamExt;

#[derive(Default)]
struct Tui {
    quit: bool,
    scrollback: Vec<u8>,
}

/// TODO(evan): Document
pub async fn run_tui(
    mut shutdown: ShutdownHandle,
    mut ghci_reader: DuplexStream,
) -> miette::Result<()> {
    let mut tracing_reader = AsyncBufferRedirect::stderr()
        .into_diagnostic()
        .wrap_err("Failed to capture stderr")?;

    let mut ghci_buffer = [0; 1024];
    let mut tracing_buffer = [0; 1024];

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

    Paragraph::new(text)
        .wrap(Wrap::default())
        .render(area, buffer);

    Ok(())
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
