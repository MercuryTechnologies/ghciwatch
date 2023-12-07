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
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use tokio_stream::StreamExt;

#[derive(Default)]
struct Tui {
    quit: bool,
}

/// TODO(evan): Document
pub async fn run_tui(mut shutdown: ShutdownHandle) -> miette::Result<()> {
    let mut terminal = terminal::enter()?;

    let mut tui = Tui::default();

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
            terminal_event = event_stream.next() => {
                let terminal_event = terminal_event
                    .ok_or_else(|| miette!("No more crossterm events"))?
                    .into_diagnostic()
                    .wrap_err("Failed to get next crossterm event")?;
                handle_event(&mut tui, terminal_event)?;
            }
        }
    }

    let _ = shutdown.request_shutdown();

    Ok(())
}

fn render(_tui: &Tui, area: Rect, buffer: &mut Buffer) {
    Paragraph::new("Hello, world!").render(area, buffer);
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
