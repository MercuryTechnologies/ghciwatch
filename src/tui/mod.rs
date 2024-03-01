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
use ratatui::Frame;
use std::cmp::min;
use tap::Conv;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::io::DuplexStream;
use tokio_stream::StreamExt;
use tracing::instrument;

/// State data for drawing the TUI.
#[derive(Default)]
struct Tui {
    quit: bool,
    scrollback: Vec<u8>,
    // TODO(evan): Follow output when scrolled to bottom
    scroll_offset: usize,
}

/// Start the terminal event loop, reading output from the given readers.
#[instrument(level = "debug", skip_all)]
pub async fn run_tui(
    mut shutdown: ShutdownHandle,
    ghci_reader: DuplexStream,
    tracing_reader: DuplexStream,
) -> miette::Result<()> {
    let mut ghci_reader = BufReader::new(ghci_reader).lines();
    let mut tracing_reader = BufReader::new(tracing_reader).lines();

    let mut terminal = terminal::enter()?;

    let mut tui = Tui::default();

    let mut event_stream = EventStream::new();

    tracing::warn!("`--tui` mode is experimental and may contain bugs or change drastically in future releases.");

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

            line = ghci_reader.next_line() => {
                let line = line.into_diagnostic().wrap_err("Failed to read line from GHCI")?;
                match line {
                    Some(line) => {
                        tui.scrollback.extend(line.bytes());
                        tui.scrollback.push(b'\n');
                    },
                    None => {
                        tui.quit = true;
                    },
                }
            }

            line = tracing_reader.next_line() => {
                let line = line.into_diagnostic().wrap_err("Failed to read line from tracing")?;
                if let Some(line) = line {
                    tui.scrollback.extend(line.bytes());
                    tui.scrollback.push(b'\n');
                }
            }

            output = event_stream.next() => {
                let event = output
                    .ok_or_else(|| miette!("No more crossterm events"))?
                    .into_diagnostic()
                    .wrap_err("Failed to get next crossterm event")?;
                // TODO: `get_frame` is an expensive call, delay if possible.
                // https://github.com/MercuryTechnologies/ghciwatch/pull/206#discussion_r1508364135
                handle_event(&mut tui, event, terminal.get_frame())?;
            }
        }
    }

    let _ = shutdown.request_shutdown();

    Ok(())
}

#[instrument(level = "trace", skip_all)]
fn render(tui: &Tui, area: Rect, buffer: &mut Buffer) -> miette::Result<()> {
    if area.width == 0 || area.height == 0 {
        return Ok(());
    }

    let text = tui.scrollback.into_text().into_diagnostic()?;

    let scroll_offset = u16::try_from(tui.scroll_offset)
        .expect("Failed to convert `scroll_offset` from usize to u16");

    Paragraph::new(text)
        .wrap(Wrap::default())
        .scroll((scroll_offset, 0))
        .render(area, buffer);

    Ok(())
}

const SCROLL_AMOUNT: usize = 1;

#[instrument(level = "trace", skip(tui, frame))]
fn handle_event(tui: &mut Tui, event: Event, frame: Frame<'_>) -> miette::Result<()> {
    let last_line = tui
        .scrollback
        .split(|byte| *byte == b'\n')
        .count()
        .saturating_sub(1);

    // TODO: Steal Evan's declarative key matching macros?
    // https://github.com/evanrelf/indigo/blob/7a5e8e47291585cae03cdf5a7c47ad3bcd8db3e6/crates/indigo-tui/src/key/macros.rs
    match event {
        Event::Mouse(mouse) if mouse.kind == MouseEventKind::ScrollUp => {
            tui.scroll_offset = tui.scroll_offset.saturating_sub(SCROLL_AMOUNT);
        }
        Event::Mouse(mouse) if mouse.kind == MouseEventKind::ScrollDown => {
            tui.scroll_offset += SCROLL_AMOUNT;
        }
        Event::Key(key) => match key.modifiers {
            KeyModifiers::NONE => match key.code {
                KeyCode::Char('j') => {
                    tui.scroll_offset += 1;
                }
                KeyCode::Char('k') => {
                    tui.scroll_offset = tui.scroll_offset.saturating_sub(1);
                }
                KeyCode::Char('g') => {
                    tui.scroll_offset = 0;
                }
                _ => {}
            },

            #[allow(clippy::single_match)]
            KeyModifiers::SHIFT => match key.code {
                KeyCode::Char('G') => {
                    tui.scroll_offset = last_line;
                }
                _ => {}
            },

            KeyModifiers::CONTROL => match key.code {
                KeyCode::Char('u') => {
                    tui.scroll_offset = tui
                        .scroll_offset
                        .saturating_sub((frame.size().height / 2).into());
                }
                KeyCode::Char('d') => {
                    tui.scroll_offset += (frame.size().height / 2).conv::<usize>();
                }
                KeyCode::Char('e') => {
                    tui.scroll_offset += 1;
                }
                KeyCode::Char('y') => {
                    tui.scroll_offset = tui.scroll_offset.saturating_sub(1);
                }
                KeyCode::Char('c') => {
                    tui.quit = true;
                }
                _ => {}
            },

            _ => {}
        },
        _ => {}
    }

    tui.scroll_offset = min(last_line, tui.scroll_offset);

    Ok(())
}
