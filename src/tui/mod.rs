mod terminal;

use crate::async_buffer_redirect::AsyncBufferRedirect;
use crate::ShutdownHandle;
use ansi_to_tui::IntoText;
use async_dup::{Arc, Mutex};
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
use tokio_util::compat::Compat;
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tokio_util::compat::TokioAsyncReadCompatExt;

#[derive(Default)]
struct Tui {
    quit: bool,
    scrollback: Vec<u8>,
    // TODO(evan): Follow output when scrolled to bottom
    scroll_offset: usize,
}

/// TODO(evan): Document
pub async fn run_tui(
    mut shutdown: ShutdownHandle,
    ghci_reader: DuplexStream,
) -> miette::Result<()> {
    let tracing_reader = AsyncBufferRedirect::stderr()
        .into_diagnostic()
        .wrap_err("Failed to capture stderr")?;

    let ghci_reader: Compat<Arc<Mutex<Compat<DuplexStream>>>> =
        Arc::new(Mutex::new(ghci_reader.compat())).compat();

    let tracing_reader: Compat<Arc<Mutex<Compat<AsyncBufferRedirect>>>> =
        Arc::new(Mutex::new(tracing_reader.compat())).compat();

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

        let mut ghci_reader = ghci_reader.clone();
        let mut ghci_future = tokio::spawn(async move {
            let mut buffer = [0; 1024];
            let result = ghci_reader.read(&mut buffer).await;
            (result, buffer)
        });

        let mut tracing_reader = tracing_reader.clone();
        let mut tracing_future = tokio::spawn(async move {
            let mut buffer = [0; 1024];
            let result = tracing_reader.read(&mut buffer).await;
            (result, buffer)
        });

        tokio::select! {
            _ = shutdown.on_shutdown_requested() => {
                tui.quit = true;
            }

            task_result = &mut ghci_future => {
                let (read_result, buffer) = task_result
                    .into_diagnostic()
                    .wrap_err("GHCi read task failed to execute to completion")?;
                let n = read_result
                    .into_diagnostic()
                    .wrap_err("Failed to read bytes from GHCi into TUI buffer")?;
                if n == 0 {
                    tui.quit = true;
                } else {
                    tui.scrollback.extend(buffer);
                }
            }

            task_result = &mut tracing_future => {
                let (read_result, buffer) = task_result
                    .into_diagnostic()
                    .wrap_err("tracing read task failed to execute to completion")?;
                read_result
                    .into_diagnostic()
                    .wrap_err("Failed to read bytes from tracing into TUI buffer")?;
                tui.scrollback.extend(buffer);
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
