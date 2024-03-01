use std::cmp::min;
use std::ops::Deref;
use std::ops::DerefMut;

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
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::io::DuplexStream;
use tokio_stream::StreamExt;
use tracing::instrument;

mod terminal;

use crate::buffers::TUI_SCROLLBACK_CAPACITY;
use crate::ShutdownHandle;
use terminal::TerminalGuard;

/// Default amount to scroll on mouse wheel events.
const SCROLL_AMOUNT: usize = 3;

/// State data for drawing the TUI.
#[derive(Debug)]
struct TuiState {
    quit: bool,
    scrollback: Vec<u8>,
    line_count: usize,
    scroll_offset: usize,
}

impl Default for TuiState {
    fn default() -> Self {
        Self {
            quit: false,
            scrollback: Vec::with_capacity(TUI_SCROLLBACK_CAPACITY),
            line_count: 1,
            scroll_offset: 0,
        }
    }
}

impl TuiState {
    #[instrument(level = "trace", skip_all)]
    fn render_inner(&self, area: Rect, buffer: &mut Buffer) -> miette::Result<()> {
        if area.width == 0 || area.height == 0 {
            return Ok(());
        }

        let text = self.scrollback.into_text().into_diagnostic()?;

        let scroll_offset = u16::try_from(self.scroll_offset).unwrap();

        Paragraph::new(text)
            .wrap(Wrap::default())
            .scroll((scroll_offset, 0))
            .render(area, buffer);

        Ok(())
    }
}

struct Tui {
    terminal: TerminalGuard,
    /// The last terminal size seen. This is updated on every `render` call.
    size: Rect,
    state: TuiState,
}

impl Deref for Tui {
    type Target = TuiState;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

impl DerefMut for Tui {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.state
    }
}

impl Tui {
    fn new(mut terminal: TerminalGuard) -> Self {
        let area = terminal.get_frame().size();
        Self {
            terminal,
            size: area,
            state: Default::default(),
        }
    }

    fn half_height(&mut self) -> usize {
        (self.size.height / 2) as usize
    }

    fn scroll_up(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    fn scroll_down(&mut self, amount: usize) {
        self.scroll_offset = min(self.scroll_max(), self.scroll_offset.saturating_add(amount));
    }

    fn scroll_max(&mut self) -> usize {
        self.line_count.saturating_sub(self.half_height())
    }

    fn scroll_to(&mut self, scroll_offset: usize) {
        self.scroll_offset = min(self.scroll_max(), scroll_offset);
    }

    fn push_line(&mut self, line: String) {
        self.scrollback.extend(line.into_bytes());
        self.scrollback.push(b'\n');
        self.line_count += 1;
    }

    #[instrument(level = "trace", skip(self))]
    fn render(&mut self) -> miette::Result<()> {
        let mut render_result = Ok(());
        self.terminal
            .draw(|frame| {
                self.size = frame.size();
                let buffer = frame.buffer_mut();
                render_result = self.state.render_inner(self.size, buffer);
            })
            .into_diagnostic()
            .wrap_err("Failed to draw to terminal")?;

        Ok(())
    }

    #[instrument(level = "trace", skip(self))]
    fn handle_event(&mut self, event: Event) -> miette::Result<()> {
        // TODO: Steal Evan's declarative key matching macros?
        // https://github.com/evanrelf/indigo/blob/7a5e8e47291585cae03cdf5a7c47ad3bcd8db3e6/crates/indigo-tui/src/key/macros.rs
        match event {
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::ScrollUp => {
                self.scroll_up(SCROLL_AMOUNT);
            }
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::ScrollDown => {
                self.scroll_down(SCROLL_AMOUNT);
            }
            Event::Key(key) => match key.modifiers {
                KeyModifiers::NONE => match key.code {
                    KeyCode::Char('j') => {
                        self.scroll_down(1);
                    }
                    KeyCode::Char('k') => {
                        self.scroll_up(1);
                    }
                    KeyCode::Char('g') => {
                        self.scroll_to(0);
                    }
                    _ => {}
                },

                #[allow(clippy::single_match)]
                KeyModifiers::SHIFT => match key.code {
                    KeyCode::Char('G') => {
                        self.scroll_to(usize::MAX);
                    }
                    _ => {}
                },

                KeyModifiers::CONTROL => match key.code {
                    KeyCode::Char('u') => {
                        let half_height = self.half_height();
                        self.scroll_up(half_height);
                    }
                    KeyCode::Char('d') => {
                        let half_height = self.half_height();
                        self.scroll_down(half_height);
                    }
                    KeyCode::Char('e') => {
                        self.scroll_down(1);
                    }
                    KeyCode::Char('y') => {
                        self.scroll_up(1);
                    }
                    KeyCode::Char('c') => {
                        self.quit = true;
                    }
                    _ => {}
                },

                _ => {}
            },
            _ => {}
        }

        Ok(())
    }
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

    let terminal = terminal::enter()?;
    let mut tui = Tui::new(terminal);

    let mut event_stream = EventStream::new();

    tracing::warn!("`--tui` mode is experimental and may contain bugs or change drastically in future releases.");

    while !tui.quit {
        tui.render()?;

        tokio::select! {
            _ = shutdown.on_shutdown_requested() => {
                tui.quit = true;
            }

            line = ghci_reader.next_line() => {
                let line = line.into_diagnostic().wrap_err("Failed to read line from GHCI")?;
                match line {
                    Some(line) => {
                        tui.push_line(line);
                    },
                    None => {
                        tui.quit = true;
                    },
                }
            }

            line = tracing_reader.next_line() => {
                let line = line.into_diagnostic().wrap_err("Failed to read line from tracing")?;
                if let Some(line) = line {
                    tui.push_line(line);
                }
            }

            output = event_stream.next() => {
                let event = output
                    .ok_or_else(|| miette!("No more crossterm events"))?
                    .into_diagnostic()
                    .wrap_err("Failed to get next crossterm event")?;
                // TODO: `get_frame` is an expensive call, delay if possible.
                // https://github.com/MercuryTechnologies/ghciwatch/pull/206#discussion_r1508364135
                tui.handle_event(event)?;
            }
        }
    }

    let _ = shutdown.request_shutdown();

    Ok(())
}
