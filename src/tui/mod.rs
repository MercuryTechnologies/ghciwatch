use std::collections::VecDeque;
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
use ratatui::prelude::Constraint;
use ratatui::prelude::Layout;
use ratatui::prelude::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::block::Title;
use ratatui::widgets::BorderType;
use ratatui::widgets::LineGauge;
use ratatui::widgets::List;
use ratatui::widgets::ListItem;
use ratatui::widgets::ListState;
use ratatui::widgets::Padding;
use ratatui::widgets::Paragraph;
use ratatui::widgets::StatefulWidget;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;
use saturating::Saturating;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::io::DuplexStream;
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tracing::instrument;
use winnow::Parser as _;

pub mod events;
mod terminal;

use crate::buffers::MAX_SCROLLBACK_LINES;
use crate::ghci::parse::CompilationResult;
use crate::ghci::parse::CompilingProgress;
use crate::ghci::parse::GhcDiagnostic;
use crate::ghci::parse::ModulesLoaded;
use crate::hooks;
use crate::ShutdownHandle;
use events::TuiEvent;
use terminal::TerminalGuard;

/// Default amount to scroll on mouse wheel events.
const SCROLL_AMOUNT: usize = 3;

/// Maximum leading whitespace preserved on scrollback lines.
/// GHC can produce lines with 40-80+ spaces of indentation in "In the expression:"
/// context blocks, which creates screens of blank space in the TUI.
const MAX_LINE_INDENT: usize = 16;

/// Cap leading whitespace on a line to [`MAX_LINE_INDENT`] spaces.
fn cap_indent(line: &str) -> String {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    if indent > MAX_LINE_INDENT {
        format!("{}{trimmed}", " ".repeat(MAX_LINE_INDENT))
    } else {
        line.to_string()
    }
}

/// Remove common leading whitespace from a multi-line string,
/// preserving relative indentation between lines.
fn dedent(text: &str) -> String {
    let min_indent = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    if min_indent == 0 {
        return text.to_string();
    }
    text.lines()
        .map(|l| {
            if l.len() >= min_indent {
                &l[min_indent..]
            } else {
                l.trim_start()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Source of a line in the scrollback buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineSource {
    Ghci,
    Tracing,
}

/// The high-level compilation state for display in the status bar.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CompilationState {
    Idle,
    Compiling,
    Succeeded { modules_loaded: ModulesLoaded },
    Failed,
    Reloading,
    Restarting,
    Testing,
}

impl Default for CompilationState {
    fn default() -> Self {
        Self::Idle
    }
}

/// Structured application state received from the GHCi task via the event channel.
#[derive(Debug, Default)]
struct TuiAppState {
    compilation_state: CompilationState,
    error_count: usize,
    warning_count: usize,
    last_progress: Option<CompilingProgress>,
    diagnostics: Vec<GhcDiagnostic>,
    last_trigger: Option<String>,
}

/// State data for drawing the TUI.
///
/// Separated from [`Tui`] so it can be tested without a real terminal backend.
#[derive(Debug)]
struct TuiState {
    debug: bool,
    quit: bool,
    dirty: bool,
    show_errors: bool,
    error_scroll_idx: usize,
    show_help: bool,
    search_mode: bool,
    search_query: String,
    search_matches: Vec<usize>,
    search_match_idx: Option<usize>,
    mouse_captured: bool,
    /// Set by `handle_event` when `m` is pressed; consumed by `run_tui`
    /// to execute the crossterm enable/disable command.
    toggle_mouse: bool,
    scrollback: VecDeque<(LineSource, String)>,
    scroll_offset: Saturating<usize>,
    /// The last terminal size seen, updated on render and resize events.
    size: Rect,
    /// Structured state from the GHCi task.
    app: TuiAppState,
}

impl Default for TuiState {
    fn default() -> Self {
        Self {
            debug: false,
            quit: false,
            dirty: true,
            show_errors: false,
            error_scroll_idx: 0,
            show_help: false,
            search_mode: false,
            search_query: String::new(),
            search_matches: Vec::new(),
            search_match_idx: None,
            mouse_captured: true,
            toggle_mouse: false,
            scrollback: VecDeque::new(),
            scroll_offset: Saturating(0),
            size: Rect::default(),
            app: TuiAppState::default(),
        }
    }
}

impl TuiState {
    /// Logical line count, including an implicit initial empty line to match
    /// the scroll math from the original `Vec<u8>` implementation.
    fn line_count(&self) -> Saturating<usize> {
        Saturating(self.scrollback.len() + 1)
    }

    fn half_height(&self) -> Saturating<usize> {
        Saturating((self.size.height / 2) as usize)
    }

    fn scroll_max(&self) -> Saturating<usize> {
        self.line_count() - self.half_height()
    }

    fn clamp_scroll(&mut self) {
        self.scroll_offset = self.scroll_offset.min(self.scroll_max());
    }

    fn scroll_up(&mut self, amount: usize) {
        self.scroll_offset -= Saturating(amount);
        self.dirty = true;
    }

    fn scroll_down(&mut self, amount: usize) {
        self.scroll_offset += Saturating(amount);
        self.clamp_scroll();
        self.dirty = true;
    }

    fn scroll_to(&mut self, scroll_offset: usize) {
        self.scroll_offset = self.scroll_max().min(Saturating(scroll_offset));
        self.dirty = true;
    }

    fn maybe_follow(&mut self) {
        let height = self.size.height as usize;

        let scrolled_to_bottom =
            self.scroll_offset >= self.line_count() - Saturating(height) - Saturating(1);

        let scrollback_exceeds_height = self.line_count() > Saturating(height);

        if scrolled_to_bottom && scrollback_exceeds_height {
            self.scroll_offset += Saturating(1);
        }
    }

    fn push_line(&mut self, source: LineSource, line: String) {
        self.scrollback.push_back((source, cap_indent(&line)));

        if self.scrollback.len() > MAX_SCROLLBACK_LINES {
            self.scrollback.pop_front();
            self.scroll_offset -= Saturating(1);
        }

        self.maybe_follow();
        self.dirty = true;
    }

    fn handle_tui_event(&mut self, event: TuiEvent) {
        match event {
            TuiEvent::CompilationStarted { changed_paths } => {
                self.app.compilation_state = CompilationState::Compiling;
                self.app.last_progress = None;
                self.app.diagnostics.clear();
                self.app.error_count = 0;
                self.app.warning_count = 0;
                self.app.last_trigger = if changed_paths.len() == 1 {
                    Some(format!("{} changed", changed_paths[0]))
                } else if !changed_paths.is_empty() {
                    Some(format!("{} files changed", changed_paths.len()))
                } else {
                    None
                };
            }
            TuiEvent::CompilationProgress(progress) => {
                self.app.last_progress = Some(progress);
            }
            TuiEvent::CompilationFinished {
                summary,
                diagnostics,
            } => {
                self.app.last_progress = None;
                self.app.error_count = diagnostics
                    .iter()
                    .filter(|d| d.severity == crate::ghci::parse::Severity::Error)
                    .count();
                self.app.warning_count = diagnostics
                    .iter()
                    .filter(|d| d.severity == crate::ghci::parse::Severity::Warning)
                    .count();
                let mut sorted = diagnostics;
                sorted.sort_by(|a, b| a.path.cmp(&b.path));
                self.app.diagnostics = sorted;
                self.error_scroll_idx = 0;

                match summary.result {
                    CompilationResult::Ok => {
                        self.app.compilation_state = CompilationState::Succeeded {
                            modules_loaded: summary.modules_loaded,
                        };
                    }
                    CompilationResult::Err => {
                        self.app.compilation_state = CompilationState::Failed;
                    }
                }
            }
            TuiEvent::Lifecycle(event) => match event {
                hooks::LifecycleEvent::Reload(hooks::When::Before) => {
                    self.app.compilation_state = CompilationState::Reloading;
                }
                hooks::LifecycleEvent::Restart(hooks::When::Before) => {
                    self.app.compilation_state = CompilationState::Restarting;
                }
                hooks::LifecycleEvent::Startup(hooks::When::After) => {
                    if self.app.compilation_state == CompilationState::Restarting {
                        self.app.compilation_state = CompilationState::Idle;
                    }
                }
                hooks::LifecycleEvent::Test => {
                    self.app.compilation_state = CompilationState::Testing;
                }
                _ => {}
            },
            TuiEvent::Clear => {
                self.scrollback.push_back((
                    LineSource::Ghci,
                    "─── Reload ───────────────────────────".to_string(),
                ));
            }
        }
        self.dirty = true;
    }

    /// Build the visible text by extracting only the lines in the current viewport.
    /// This avoids converting the entire scrollback through `ansi-to-tui` on every frame
    /// and eliminates the u16 scroll offset limitation of `Paragraph::scroll()`.
    /// Tracing lines are prefixed with ANSI dim escape codes.
    fn visible_text(&self, viewport_height: usize) -> String {
        let start = self.scroll_offset.0;
        let end = (start + viewport_height).min(self.scrollback.len());

        self.scrollback
            .iter()
            .skip(start)
            .take(end.saturating_sub(start))
            .map(|(source, line)| match source {
                LineSource::Tracing => format!("\x1b[2m{line}\x1b[0m"),
                LineSource::Ghci => line.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn render_header_line(&self) -> Line<'_> {
        let sep = Span::styled(" | ", Style::default().fg(Color::DarkGray));
        let app_name = Span::styled("ghciwatch", Style::default().add_modifier(Modifier::BOLD));

        let state_span = match &self.app.compilation_state {
            CompilationState::Idle => Span::styled("Idle", Style::default().fg(Color::DarkGray)),
            CompilationState::Compiling => {
                if let Some(p) = &self.app.last_progress {
                    Span::styled(
                        format!("Compiling [{}/{}] {}", p.current, p.total, p.module.name),
                        Style::default().fg(Color::Yellow),
                    )
                } else {
                    Span::styled("Compiling...", Style::default().fg(Color::Yellow))
                }
            }
            CompilationState::Succeeded { modules_loaded } => Span::styled(
                format!("OK ({modules_loaded} modules)"),
                Style::default().fg(Color::Green),
            ),
            CompilationState::Failed => {
                Span::styled("FAILED", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
            }
            CompilationState::Reloading => {
                Span::styled("Reloading...", Style::default().fg(Color::Yellow))
            }
            CompilationState::Restarting => {
                Span::styled("Restarting...", Style::default().fg(Color::Magenta))
            }
            CompilationState::Testing => {
                Span::styled("Testing...", Style::default().fg(Color::Cyan))
            }
        };

        let mut spans = vec![app_name, sep.clone(), state_span];

        if self.app.error_count > 0 || self.app.warning_count > 0 {
            spans.push(sep.clone());
            if self.app.error_count > 0 {
                spans.push(Span::styled(
                    format!("{} errors", self.app.error_count),
                    Style::default().fg(Color::Red),
                ));
                if self.app.warning_count > 0 {
                    spans.push(Span::raw(", "));
                }
            }
            if self.app.warning_count > 0 {
                spans.push(Span::styled(
                    format!("{} warnings", self.app.warning_count),
                    Style::default().fg(Color::Yellow),
                ));
            }
        }

        if let Some(trigger) = &self.app.last_trigger {
            spans.push(sep);
            spans.push(Span::styled(
                trigger.clone(),
                Style::default().fg(Color::DarkGray),
            ));
        }

        Line::from(spans)
    }

    fn render_footer_line(&self, content_area_height: u16) -> Line<'_> {
        let hint_style = Style::default().fg(Color::DarkGray);

        if self.search_mode {
            return Line::from(vec![
                Span::styled("/", Style::default().fg(Color::Yellow)),
                Span::raw(&self.search_query),
                Span::styled("█", Style::default().fg(Color::Yellow)),
                Span::styled("  Enter confirm  Esc cancel", hint_style),
            ]);
        }

        let mut hints_text = String::from(
            "j/k scroll  g/G top/end  ^u/^d half  PgUp/Dn page  / search  ? help",
        );
        if !self.app.diagnostics.is_empty() {
            hints_text.push_str("  e errors");
        }
        if !self.search_matches.is_empty() {
            hints_text.push_str("  n/N next/prev");
        }
        if self.mouse_captured {
            hints_text.push_str("  m select");
        } else {
            hints_text.push_str("  m mouse");
        }
        hints_text.push_str("  q quit");

        let hints = Span::styled(hints_text, hint_style);

        let total = self.scrollback.len();
        let pos_text = if total > 0 {
            let viewport_bottom = self
                .scroll_offset
                .0
                .saturating_add(content_area_height as usize)
                .min(total);
            let pct = (viewport_bottom * 100) / total;
            format!(" {viewport_bottom}/{total} ({pct}%)")
        } else {
            String::new()
        };

        let pos = Span::styled(pos_text, hint_style);

        Line::from(vec![hints, pos])
    }

    #[instrument(level = "trace", skip_all)]
    fn render_inner(&self, area: Rect, buffer: &mut Buffer) -> miette::Result<()> {
        if area.width == 0 || area.height == 0 {
            return Ok(());
        }

        let show_header = area.height >= 5;
        let show_footer = area.height >= 10;

        let areas = Layout::vertical([
            Constraint::Length(if show_header { 1 } else { 0 }),
            Constraint::Fill(1),
            Constraint::Length(if show_footer { 1 } else { 0 }),
        ])
        .split(area);

        let header_area = areas[0];
        let content_area = areas[1];
        let footer_area = areas[2];

        if show_header {
            if let (CompilationState::Compiling, Some(p)) =
                (&self.app.compilation_state, &self.app.last_progress)
            {
                let ratio = (p.current as f64 / p.total.max(1) as f64).min(1.0);

                let header = self.render_header_line();
                let label_width = header
                    .spans
                    .iter()
                    .map(|s| s.content.len() as u16)
                    .sum::<u16>()
                    + 2;

                let header_areas = Layout::horizontal([
                    Constraint::Length(label_width.min(header_area.width / 2)),
                    Constraint::Fill(1),
                ])
                .split(header_area);

                Paragraph::new(header).render(header_areas[0], buffer);

                let gauge_label = format!(" {}/{} ", p.current, p.total);
                LineGauge::default()
                    .ratio(ratio)
                    .label(gauge_label)
                    .gauge_style(
                        Style::default()
                            .fg(Color::Green)
                            .bg(Color::Indexed(238)),
                    )
                    .render(header_areas[1], buffer);
            } else {
                let header = self.render_header_line();
                Paragraph::new(header).render(header_area, buffer);
            }
        }

        if self.show_help {
            let help_text = vec![
                Line::from(Span::styled(
                    "Keybindings",
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from("  j / k          Scroll down / up one line"),
                Line::from("  g / G          Scroll to top / bottom"),
                Line::from("  Ctrl+u / d     Scroll up / down half page"),
                Line::from("  Ctrl+e / y     Scroll down / up one line"),
                Line::from("  PgUp / PgDn    Scroll up / down full page"),
                Line::from("  /              Search in scrollback"),
                Line::from("  n / N          Next / previous search match"),
                Line::from("  e              Toggle error/warning overlay"),
                Line::from("  m              Toggle mouse (off = text selection)"),
                Line::from("  Shift+Click    Select text (works with mouse on)"),
                Line::from("  q              Quit (or close overlay)"),
                Line::from("  Ctrl+c         Quit immediately"),
                Line::from("  Esc            Close overlay / cancel search"),
                Line::from("  ? / F1         Toggle this help"),
            ];

            let block = ratatui::widgets::Block::default()
                .title(" Help [?/Esc to close] ")
                .borders(ratatui::widgets::Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));

            Paragraph::new(help_text)
                .block(block)
                .render(content_area, buffer);
        } else if self.show_errors && !self.app.diagnostics.is_empty() {
            let body_style = Style::default().fg(Color::Gray);
            let sep_style = Style::default().fg(Color::Indexed(243));
            let diag_count = self.app.diagnostics.len();

            let items: Vec<ListItem> = self
                .app
                .diagnostics
                .iter()
                .enumerate()
                .map(|(idx, d)| {
                    let (badge_style, sev_label) = match d.severity {
                        crate::ghci::parse::Severity::Error => (
                            Style::default()
                                .fg(Color::White)
                                .bg(Color::Red)
                                .add_modifier(Modifier::BOLD),
                            " error ",
                        ),
                        crate::ghci::parse::Severity::Warning => (
                            Style::default()
                                .fg(Color::Black)
                                .bg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                            " warning ",
                        ),
                    };

                    let mut lines = Vec::new();

                    let location = match (&d.path, d.span.is_zero()) {
                        (Some(path), true) => format!("  {path}"),
                        (Some(path), false) => format!("  {path}:{}", d.span),
                        (None, _) => String::new(),
                    };

                    lines.push(Line::from(vec![
                        Span::styled(sev_label, badge_style),
                        Span::styled(location, Style::default().fg(Color::White)),
                    ]));

                    let dedented = dedent(&d.message);
                    for msg_line in dedented.lines() {
                        lines.push(Line::from(Span::styled(
                            format!("   {msg_line}"),
                            body_style,
                        )));
                    }

                    if idx + 1 < diag_count {
                        lines.push(Line::from(Span::styled(
                            "   ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─",
                            sep_style,
                        )));
                    }

                    ListItem::new(lines)
                })
                .collect();

            let top_title = format!(
                " {} errors, {} warnings ",
                self.app.error_count, self.app.warning_count
            );
            let block = ratatui::widgets::Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Indexed(240)))
                .title(top_title)
                .title(
                    Title::from(Span::styled(
                        " j/k navigate | e/Esc close ",
                        Style::default().fg(Color::Indexed(245)),
                    ))
                    .position(ratatui::widgets::block::Position::Bottom),
                )
                .padding(Padding::new(1, 1, 0, 0));

            let list = List::new(items)
                .block(block)
                .highlight_style(Style::default().bg(Color::Indexed(238)))
                .highlight_symbol("▎ ");

            let mut list_state = ListState::default();
            list_state.select(Some(self.error_scroll_idx));
            StatefulWidget::render(list, content_area, buffer, &mut list_state);
        } else {
            let viewport_height = content_area.height as usize;
            let visible = self.visible_text(viewport_height);
            let text = visible.as_bytes().into_text().into_diagnostic()?;

            Paragraph::new(text)
                .wrap(Wrap::default())
                .render(content_area, buffer);
        }

        if show_footer {
            let footer = self.render_footer_line(content_area.height);
            Paragraph::new(footer).render(footer_area, buffer);
        }

        Ok(())
    }

    fn execute_search(&mut self) {
        self.search_matches.clear();
        self.search_match_idx = None;

        if self.search_query.is_empty() {
            return;
        }

        let query = self.search_query.to_lowercase();
        for (i, (_, line)) in self.scrollback.iter().enumerate() {
            if line.to_lowercase().contains(&query) {
                self.search_matches.push(i);
            }
        }

        if let Some(&first) = self.search_matches.first() {
            self.search_match_idx = Some(0);
            self.scroll_offset = Saturating(first);
        }
    }

    fn search_next(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        let idx = self.search_match_idx.map(|i| (i + 1) % self.search_matches.len()).unwrap_or(0);
        self.search_match_idx = Some(idx);
        self.scroll_offset = Saturating(self.search_matches[idx]);
        self.dirty = true;
    }

    fn search_prev(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        let len = self.search_matches.len();
        let idx = self
            .search_match_idx
            .map(|i| (i + len - 1) % len)
            .unwrap_or(len - 1);
        self.search_match_idx = Some(idx);
        self.scroll_offset = Saturating(self.search_matches[idx]);
        self.dirty = true;
    }

    fn handle_search_event(&mut self, event: Event) {
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Esc => {
                    self.search_mode = false;
                    self.dirty = true;
                }
                KeyCode::Enter => {
                    self.search_mode = false;
                    self.execute_search();
                    self.dirty = true;
                }
                KeyCode::Backspace => {
                    self.search_query.pop();
                    self.dirty = true;
                }
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                    self.dirty = true;
                }
                _ => {}
            }
        }
    }

    #[instrument(level = "trace", skip(self))]
    fn handle_event(&mut self, event: Event) -> miette::Result<()> {
        if self.search_mode {
            self.handle_search_event(event);
            return Ok(());
        }

        match event {
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => self.scroll_up(SCROLL_AMOUNT),
                MouseEventKind::ScrollDown => self.scroll_down(SCROLL_AMOUNT),
                _ => {}
            },
            Event::Key(key) => match (key.modifiers, key.code) {
                (KeyModifiers::NONE, KeyCode::Char('j')) if self.show_errors => {
                    if self.error_scroll_idx + 1 < self.app.diagnostics.len() {
                        self.error_scroll_idx += 1;
                        self.dirty = true;
                    }
                }
                (KeyModifiers::NONE, KeyCode::Char('k')) if self.show_errors => {
                    self.error_scroll_idx = self.error_scroll_idx.saturating_sub(1);
                    self.dirty = true;
                }
                (KeyModifiers::NONE, KeyCode::Char('j')) => self.scroll_down(1),
                (KeyModifiers::NONE, KeyCode::Char('k')) => self.scroll_up(1),
                (KeyModifiers::NONE, KeyCode::Char('g')) => self.scroll_to(0),
                (KeyModifiers::SHIFT, KeyCode::Char('g' | 'G')) => self.scroll_to(usize::MAX),
                (KeyModifiers::CONTROL, KeyCode::Char('u')) => self.scroll_up(self.half_height().0),
                (KeyModifiers::CONTROL, KeyCode::Char('d')) => {
                    self.scroll_down(self.half_height().0)
                }
                (KeyModifiers::CONTROL, KeyCode::Char('e')) => self.scroll_down(1),
                (KeyModifiers::CONTROL, KeyCode::Char('y')) => self.scroll_up(1),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => self.quit = true,
                (KeyModifiers::NONE, KeyCode::Char('q')) => {
                    if self.show_errors {
                        self.show_errors = false;
                        self.dirty = true;
                    } else {
                        self.quit = true;
                    }
                }
                (KeyModifiers::NONE, KeyCode::Char('e')) => {
                    if !self.app.diagnostics.is_empty() {
                        self.show_errors = !self.show_errors;
                        self.dirty = true;
                    }
                }
                (KeyModifiers::NONE, KeyCode::Char('/')) => {
                    self.search_mode = true;
                    self.search_query.clear();
                    self.search_matches.clear();
                    self.search_match_idx = None;
                    self.dirty = true;
                }
                (KeyModifiers::NONE, KeyCode::Char('n')) => {
                    self.search_next();
                }
                (KeyModifiers::SHIFT, KeyCode::Char('n' | 'N')) => {
                    self.search_prev();
                }
                (KeyModifiers::NONE, KeyCode::Char('m')) => {
                    self.toggle_mouse = true;
                    self.dirty = true;
                }
                (KeyModifiers::NONE, KeyCode::Char('?')) => {
                    self.show_help = !self.show_help;
                    self.dirty = true;
                }
                (KeyModifiers::NONE, KeyCode::F(1)) => {
                    self.show_help = !self.show_help;
                    self.dirty = true;
                }
                (KeyModifiers::NONE, KeyCode::Esc) => {
                    if self.show_errors {
                        self.show_errors = false;
                        self.dirty = true;
                    } else if self.show_help {
                        self.show_help = false;
                        self.dirty = true;
                    }
                }
                (KeyModifiers::NONE, KeyCode::PageUp) => {
                    let amount = self.size.height.saturating_sub(1) as usize;
                    self.scroll_up(amount);
                }
                (KeyModifiers::NONE, KeyCode::PageDown) => {
                    let amount = self.size.height.saturating_sub(1) as usize;
                    self.scroll_down(amount);
                }
                (_, KeyCode::Char('`' | '~')) => {
                    self.debug = !self.debug;
                    self.dirty = true;
                }
                _ => {}
            },
            Event::Resize(w, h) => {
                self.size = Rect::new(0, 0, w, h);
                self.clamp_scroll();
                self.dirty = true;
            }
            _ => {}
        }

        Ok(())
    }
}

struct Tui {
    terminal: TerminalGuard,
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
        let mut state = TuiState::default();
        state.size = area;
        Self { terminal, state }
    }

    #[instrument(level = "trace", skip(self))]
    fn render(&mut self) -> miette::Result<()> {
        let mut render_result = Ok(());
        self.terminal
            .draw(|frame| {
                self.state.size = frame.size();
                let buffer = frame.buffer_mut();
                render_result = self.state.render_inner(self.state.size, buffer);
            })
            .into_diagnostic()
            .wrap_err("Failed to draw to terminal")?;
        self.state.dirty = false;

        Ok(())
    }
}

/// Receive from the broadcast channel, or pend forever if no channel is provided.
async fn recv_tui_event(
    rx: &mut Option<broadcast::Receiver<TuiEvent>>,
) -> Result<TuiEvent, broadcast::error::RecvError> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

/// Start the terminal event loop, reading output from the given readers.
#[instrument(level = "debug", skip_all)]
pub async fn run_tui(
    mut shutdown: ShutdownHandle,
    ghci_reader: DuplexStream,
    tracing_reader: DuplexStream,
    mut tui_rx: Option<broadcast::Receiver<TuiEvent>>,
) -> miette::Result<()> {
    let mut ghci_reader = BufReader::new(ghci_reader).lines();
    let mut tracing_reader = BufReader::new(tracing_reader).lines();

    let terminal = terminal::enter()?;
    let mut tui = Tui::new(terminal);

    let mut event_stream = EventStream::new();

    while !tui.quit {
        if tui.dirty {
            tui.render()?;
        }

        tokio::select! {
            _ = shutdown.on_shutdown_requested() => {
                tui.quit = true;
            }

            line = ghci_reader.next_line() => {
                let line = line.into_diagnostic().wrap_err("Failed to read line from GHCI")?;
                match line {
                    Some(line) => {
                        let stripped = strip_ansi_escapes::strip_str(&line);
                        let is_compiling = crate::ghci::parse::compiling
                            .parse(&*stripped)
                            .is_ok();
                        if !is_compiling {
                            tui.push_line(LineSource::Ghci, line);
                        }
                    },
                    None => {
                        tui.quit = true;
                    },
                }
            }

            line = tracing_reader.next_line() => {
                let line = line.into_diagnostic().wrap_err("Failed to read line from tracing")?;
                if let Some(line) = line {
                    tui.push_line(LineSource::Tracing, line);
                }
            }

            result = recv_tui_event(&mut tui_rx) => {
                match result {
                    Ok(event) => {
                        tui.handle_tui_event(event);
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("TUI event channel lagged, missed {n} events");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        tracing::debug!("TUI event channel closed");
                    }
                }
            }

            output = event_stream.next() => {
                let event = output
                    .ok_or_else(|| miette!("No more crossterm events"))?
                    .into_diagnostic()
                    .wrap_err("Failed to get next crossterm event")?;
                tui.handle_event(event)?;
            }
        }

        if tui.toggle_mouse {
            tui.toggle_mouse = false;
            tui.mouse_captured = !tui.mouse_captured;
            let mut stdout = std::io::stdout();
            if tui.mouse_captured {
                crossterm::execute!(stdout, crossterm::event::EnableMouseCapture)
                    .into_diagnostic()
                    .wrap_err("Failed to enable mouse capture")?;
            } else {
                crossterm::execute!(stdout, crossterm::event::DisableMouseCapture)
                    .into_diagnostic()
                    .wrap_err("Failed to disable mouse capture")?;
            }
        }
    }

    let _ = shutdown.request_shutdown();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyEventKind;
    use crossterm::event::KeyEventState;
    use pretty_assertions::assert_eq;

    fn make_state(width: u16, height: u16) -> TuiState {
        let mut state = TuiState::default();
        state.size = Rect::new(0, 0, width, height);
        state
    }

    fn render_to_lines(state: &TuiState) -> Vec<String> {
        let area = Rect::new(0, 0, state.size.width, state.size.height);
        let mut buffer = Buffer::empty(area);
        state.render_inner(area, &mut buffer).unwrap();
        let mut lines = Vec::new();
        for y in 0..state.size.height {
            let mut line = String::new();
            for x in 0..state.size.width {
                line.push_str(buffer.get(x, y).symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        lines
    }

    fn key_event(modifiers: KeyModifiers, code: KeyCode) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        })
    }

    // --- Baseline rendering tests ---

    #[test]
    fn empty_state_renders() {
        let state = make_state(60, 12);
        let lines = render_to_lines(&state);
        assert_eq!(lines.len(), 12);
        assert!(lines[0].contains("ghciwatch"));
        // Content area should be blank
        for line in &lines[1..11] {
            assert_eq!(line, "");
        }
    }

    #[test]
    fn push_line_appears_in_render() {
        let mut state = make_state(60, 12);
        state.push_line(LineSource::Ghci, "hello world".into());
        let lines = render_to_lines(&state);
        // line 0 = header, line 1 = first content line
        assert_eq!(lines[1], "hello world");
    }

    #[test]
    fn header_renders_idle_state() {
        let state = make_state(60, 10);
        let lines = render_to_lines(&state);
        assert!(lines[0].contains("ghciwatch"));
        assert!(lines[0].contains("Idle"));
    }

    #[test]
    fn header_renders_ok_state() {
        let mut state = make_state(80, 10);
        state.app.compilation_state = CompilationState::Succeeded {
            modules_loaded: ModulesLoaded::Count(42),
        };
        let lines = render_to_lines(&state);
        assert!(lines[0].contains("OK"));
        assert!(lines[0].contains("42"));
    }

    #[test]
    fn header_renders_failed_state() {
        let mut state = make_state(80, 10);
        state.app.compilation_state = CompilationState::Failed;
        state.app.error_count = 3;
        state.app.warning_count = 1;
        let lines = render_to_lines(&state);
        assert!(lines[0].contains("FAILED"));
        assert!(lines[0].contains("3 errors"));
        assert!(lines[0].contains("1 warnings"));
    }

    #[test]
    fn footer_renders_with_keyhints() {
        let mut state = make_state(100, 12);
        for i in 0..50 {
            state.push_line(LineSource::Ghci, format!("line {i}"));
        }
        let lines = render_to_lines(&state);
        let footer = &lines[11];
        assert!(footer.contains("j/k scroll"));
        assert!(footer.contains("q quit"));
    }

    // --- Resize handling tests ---

    #[test]
    fn resize_clamps_scroll() {
        let mut state = make_state(80, 40);
        for i in 0..100 {
            state.push_line(LineSource::Ghci, format!("line {i}"));
        }
        state.scroll_to(80);

        state
            .handle_event(Event::Resize(80, 10))
            .unwrap();

        assert!(state.scroll_offset <= state.scroll_max());
    }

    #[test]
    fn resize_updates_size() {
        let mut state = make_state(80, 40);
        state
            .handle_event(Event::Resize(120, 50))
            .unwrap();
        assert_eq!(state.size.width, 120);
        assert_eq!(state.size.height, 50);
    }

    // --- Keybinding tests ---

    #[test]
    fn q_key_quits() {
        let mut state = make_state(40, 10);
        state
            .handle_event(key_event(KeyModifiers::NONE, KeyCode::Char('q')))
            .unwrap();
        assert!(state.quit);
    }

    #[test]
    fn ctrl_c_quits() {
        let mut state = make_state(40, 10);
        state
            .handle_event(key_event(KeyModifiers::CONTROL, KeyCode::Char('c')))
            .unwrap();
        assert!(state.quit);
    }

    #[test]
    fn page_down_scrolls_full_page() {
        let mut state = make_state(40, 20);
        for i in 0..100 {
            state.push_line(LineSource::Ghci, format!("line {i}"));
        }
        state.scroll_to(50);
        let before = state.scroll_offset;

        state
            .handle_event(key_event(KeyModifiers::NONE, KeyCode::PageDown))
            .unwrap();
        assert_eq!(state.scroll_offset, before + Saturating(19));
    }

    #[test]
    fn page_up_scrolls_full_page() {
        let mut state = make_state(40, 20);
        for i in 0..100 {
            state.push_line(LineSource::Ghci, format!("line {i}"));
        }
        state.scroll_to(50);

        state
            .handle_event(key_event(KeyModifiers::NONE, KeyCode::PageUp))
            .unwrap();
        assert_eq!(state.scroll_offset, Saturating(50_usize) - Saturating(19_usize));
    }

    #[test]
    fn backtick_toggles_debug() {
        let mut state = make_state(40, 10);
        assert!(!state.debug);

        state
            .handle_event(key_event(KeyModifiers::NONE, KeyCode::Char('`')))
            .unwrap();
        assert!(state.debug);

        state
            .handle_event(key_event(KeyModifiers::NONE, KeyCode::Char('`')))
            .unwrap();
        assert!(!state.debug);
    }

    #[test]
    fn tilde_toggles_debug() {
        let mut state = make_state(40, 10);
        assert!(!state.debug);

        state
            .handle_event(key_event(KeyModifiers::SHIFT, KeyCode::Char('~')))
            .unwrap();
        assert!(state.debug);

        state
            .handle_event(key_event(KeyModifiers::SHIFT, KeyCode::Char('~')))
            .unwrap();
        assert!(!state.debug);
    }

    #[test]
    fn scroll_down_then_up() {
        let mut state = make_state(40, 5);
        for i in 0..20 {
            state.push_line(LineSource::Ghci, format!("line {i}"));
        }
        state.scroll_to(0);
        assert_eq!(state.scroll_offset, Saturating(0));

        state.scroll_down(3);
        assert_eq!(state.scroll_offset, Saturating(3));

        state.scroll_up(1);
        assert_eq!(state.scroll_offset, Saturating(2));
    }

    #[test]
    fn scroll_offset_clamped_to_max() {
        let mut state = make_state(40, 10);
        for i in 0..20 {
            state.push_line(LineSource::Ghci, format!("line {i}"));
        }
        state.scroll_down(999);
        assert!(state.scroll_offset <= state.scroll_max());
    }

    #[test]
    fn g_scrolls_to_top() {
        let mut state = make_state(40, 10);
        for i in 0..50 {
            state.push_line(LineSource::Ghci, format!("line {i}"));
        }
        state.scroll_down(20);
        assert!(state.scroll_offset > Saturating(0));

        state
            .handle_event(key_event(KeyModifiers::NONE, KeyCode::Char('g')))
            .unwrap();
        assert_eq!(state.scroll_offset, Saturating(0));
    }

    #[test]
    fn shift_g_scrolls_to_bottom() {
        let mut state = make_state(40, 10);
        for i in 0..50 {
            state.push_line(LineSource::Ghci, format!("line {i}"));
        }
        state.scroll_to(0);

        state
            .handle_event(key_event(KeyModifiers::SHIFT, KeyCode::Char('G')))
            .unwrap();
        assert_eq!(state.scroll_offset, state.scroll_max());
    }

    // --- Dirty flag tests ---

    #[test]
    fn dirty_flag_initially_true() {
        let state = make_state(40, 10);
        assert!(state.dirty);
    }

    #[test]
    fn dirty_flag_set_on_push_line() {
        let mut state = make_state(40, 10);
        state.dirty = false;
        state.push_line(LineSource::Ghci, "test".into());
        assert!(state.dirty);
    }

    #[test]
    fn dirty_flag_set_on_scroll() {
        let mut state = make_state(40, 10);
        for i in 0..20 {
            state.push_line(LineSource::Ghci, format!("line {i}"));
        }
        state.dirty = false;
        state.scroll_down(1);
        assert!(state.dirty);
    }

    #[test]
    fn dirty_flag_set_on_scroll_up() {
        let mut state = make_state(40, 10);
        for i in 0..20 {
            state.push_line(LineSource::Ghci, format!("line {i}"));
        }
        state.scroll_down(5);
        state.dirty = false;
        state.scroll_up(1);
        assert!(state.dirty);
    }

    #[test]
    fn dirty_flag_not_set_for_unrecognized_key() {
        let mut state = make_state(40, 10);
        state.dirty = false;
        state
            .handle_event(key_event(KeyModifiers::NONE, KeyCode::Char('z')))
            .unwrap();
        assert!(!state.dirty);
    }

    #[test]
    fn dirty_flag_set_on_resize() {
        let mut state = make_state(40, 10);
        state.dirty = false;
        state.handle_event(Event::Resize(80, 24)).unwrap();
        assert!(state.dirty);
    }

    #[test]
    fn dirty_flag_set_on_debug_toggle() {
        let mut state = make_state(40, 10);
        state.dirty = false;
        state
            .handle_event(key_event(KeyModifiers::NONE, KeyCode::Char('`')))
            .unwrap();
        assert!(state.dirty);
    }

    // --- Auto-follow tests ---

    #[test]
    fn auto_follow_when_at_bottom() {
        let mut state = make_state(40, 10);
        for i in 0..15 {
            state.push_line(LineSource::Ghci, format!("line {i}"));
        }
        let offset_before = state.scroll_offset;
        state.push_line(LineSource::Ghci, "new line".into());
        assert!(state.scroll_offset > offset_before);
    }

    #[test]
    fn no_auto_follow_when_scrolled_up() {
        let mut state = make_state(40, 10);
        for i in 0..30 {
            state.push_line(LineSource::Ghci, format!("line {i}"));
        }
        state.scroll_to(5);
        let offset_before = state.scroll_offset;
        state.push_line(LineSource::Ghci, "new line".into());
        assert_eq!(state.scroll_offset, offset_before);
    }

    // --- Phase 1: Ring buffer and viewport tests ---

    #[test]
    fn scrollback_bounded_by_max() {
        let mut state = make_state(40, 10);
        for i in 0..15_000 {
            state.push_line(LineSource::Ghci, format!("line {i}"));
        }
        assert_eq!(state.scrollback.len(), MAX_SCROLLBACK_LINES);
        assert_eq!(state.scrollback.front().unwrap().1, "line 5000");
        assert_eq!(state.scrollback.back().unwrap().1, "line 14999");
    }

    #[test]
    fn scroll_offset_adjusted_on_pruning() {
        let mut state = make_state(40, 10);
        for i in 0..MAX_SCROLLBACK_LINES {
            state.push_line(LineSource::Ghci, format!("line {i}"));
        }
        state.scroll_to(5000);
        let offset_before = state.scroll_offset;

        // Push 100 more lines, causing 100 pops from front
        for i in 0..100 {
            state.push_line(LineSource::Ghci, format!("extra {i}"));
        }
        state.scroll_to(offset_before.0);
        // The content at the old offset has shifted up
        assert!(state.scroll_offset <= state.scroll_max());
    }

    #[test]
    fn viewport_extraction_correct() {
        let mut state = make_state(40, 5);
        for i in 0..20 {
            state.push_line(LineSource::Ghci, format!("line {i}"));
        }
        state.scroll_to(5);

        let visible = state.visible_text(5);
        assert!(visible.starts_with("line 5"));
        assert!(visible.contains("line 9"));
        assert!(!visible.contains("line 10"));
    }

    #[test]
    fn viewport_at_end_of_buffer() {
        let mut state = make_state(40, 5);
        for i in 0..10 {
            state.push_line(LineSource::Ghci, format!("line {i}"));
        }
        state.scroll_to(8);

        let visible = state.visible_text(5);
        assert!(visible.contains("line 8"));
        assert!(visible.contains("line 9"));
    }

    #[test]
    fn large_scrollback_renders_without_u16_error() {
        let mut state = make_state(40, 5);
        // Push enough lines to exceed u16::MAX if we were using the old scroll method
        for i in 0..MAX_SCROLLBACK_LINES {
            state.push_line(LineSource::Ghci, format!("line {i}"));
        }
        state.scroll_to(9000);

        let area = Rect::new(0, 0, 40, 5);
        let mut buffer = Buffer::empty(area);
        let result = state.render_inner(area, &mut buffer);
        assert!(result.is_ok());
    }

    #[test]
    fn auto_follow_through_pruning() {
        let mut state = make_state(40, 10);
        // Fill the buffer to capacity
        for i in 0..MAX_SCROLLBACK_LINES {
            state.push_line(LineSource::Ghci, format!("line {i}"));
        }

        // Should be following at the bottom
        let offset_before = state.scroll_offset;
        state.push_line(LineSource::Ghci, "new after full".into());
        // Should still be following even after pruning
        assert!(state.scroll_offset >= offset_before);
        assert_eq!(state.scrollback.back().unwrap().1, "new after full");
    }

    #[test]
    fn scroll_up_stable_during_pruning() {
        let mut state = make_state(40, 10);
        for i in 0..MAX_SCROLLBACK_LINES {
            state.push_line(LineSource::Ghci, format!("line {i}"));
        }
        // Scroll to middle
        state.scroll_to(5000);
        let offset_before = state.scroll_offset;

        // Push one more line (triggers pruning)
        state.push_line(LineSource::Ghci, "extra".into());
        // Offset should have been decremented by 1 (content shifted)
        // but may also have been adjusted by maybe_follow. Since we're scrolled up,
        // maybe_follow shouldn't trigger.
        assert_eq!(state.scroll_offset.0, offset_before.0 - 1);
    }

    // --- Phase 2: TuiEvent handling tests ---

    use crate::ghci::parse::CompilationSummary;
    use crate::ghci::parse::Severity;

    fn make_diagnostic(severity: Severity, message: &str) -> GhcDiagnostic {
        GhcDiagnostic {
            severity,
            path: Some("src/Foo.hs".into()),
            span: crate::ghci::parse::PositionRange::default(),
            message: message.to_string(),
        }
    }

    #[test]
    fn compilation_started_resets_state() {
        let mut state = make_state(40, 10);
        state.app.error_count = 5;
        state.app.warning_count = 3;
        state.app.diagnostics.push(make_diagnostic(Severity::Error, "old"));

        state.handle_tui_event(TuiEvent::CompilationStarted {
            changed_paths: vec!["src/Foo.hs".to_string()],
        });

        assert_eq!(state.app.compilation_state, CompilationState::Compiling);
        assert_eq!(state.app.error_count, 0);
        assert_eq!(state.app.warning_count, 0);
        assert!(state.app.diagnostics.is_empty());
        assert!(state.app.last_progress.is_none());
    }

    #[test]
    fn compilation_finished_ok_sets_state() {
        let mut state = make_state(40, 10);
        state.handle_tui_event(TuiEvent::CompilationFinished {
            summary: CompilationSummary {
                result: CompilationResult::Ok,
                modules_loaded: ModulesLoaded::Count(123),
            },
            diagnostics: vec![
                make_diagnostic(Severity::Warning, "unused import"),
                make_diagnostic(Severity::Warning, "missing sig"),
            ],
        });

        assert_eq!(
            state.app.compilation_state,
            CompilationState::Succeeded {
                modules_loaded: ModulesLoaded::Count(123)
            }
        );
        assert_eq!(state.app.error_count, 0);
        assert_eq!(state.app.warning_count, 2);
        assert_eq!(state.app.diagnostics.len(), 2);
    }

    #[test]
    fn compilation_finished_err_sets_state() {
        let mut state = make_state(40, 10);
        state.handle_tui_event(TuiEvent::CompilationFinished {
            summary: CompilationSummary {
                result: CompilationResult::Err,
                modules_loaded: ModulesLoaded::Count(58),
            },
            diagnostics: vec![
                make_diagnostic(Severity::Error, "type mismatch"),
                make_diagnostic(Severity::Error, "not in scope"),
                make_diagnostic(Severity::Warning, "unused"),
            ],
        });

        assert_eq!(state.app.compilation_state, CompilationState::Failed);
        assert_eq!(state.app.error_count, 2);
        assert_eq!(state.app.warning_count, 1);
    }

    #[test]
    fn lifecycle_reload_sets_reloading() {
        let mut state = make_state(40, 10);
        state.handle_tui_event(TuiEvent::Lifecycle(hooks::LifecycleEvent::Reload(
            hooks::When::Before,
        )));
        assert_eq!(state.app.compilation_state, CompilationState::Reloading);
    }

    #[test]
    fn lifecycle_restart_sets_restarting() {
        let mut state = make_state(40, 10);
        state.handle_tui_event(TuiEvent::Lifecycle(hooks::LifecycleEvent::Restart(
            hooks::When::Before,
        )));
        assert_eq!(state.app.compilation_state, CompilationState::Restarting);
    }

    #[test]
    fn lifecycle_startup_after_clears_restarting() {
        let mut state = make_state(40, 10);
        state.app.compilation_state = CompilationState::Restarting;
        state.handle_tui_event(TuiEvent::Lifecycle(hooks::LifecycleEvent::Startup(
            hooks::When::After,
        )));
        assert_eq!(state.app.compilation_state, CompilationState::Idle);
    }

    #[test]
    fn lifecycle_test_sets_testing() {
        let mut state = make_state(40, 10);
        state.handle_tui_event(TuiEvent::Lifecycle(hooks::LifecycleEvent::Test));
        assert_eq!(state.app.compilation_state, CompilationState::Testing);
    }

    #[test]
    fn clear_event_inserts_separator() {
        let mut state = make_state(40, 10);
        for i in 0..5 {
            state.push_line(LineSource::Ghci, format!("line {i}"));
        }
        let len_before = state.scrollback.len();

        state.handle_tui_event(TuiEvent::Clear);

        assert_eq!(state.scrollback.len(), len_before + 1);
        assert!(state.scrollback.back().unwrap().1.contains("Reload"));
    }

    #[test]
    fn small_terminal_hides_header_and_footer() {
        let state = make_state(40, 4);
        let lines = render_to_lines(&state);
        assert_eq!(lines.len(), 4);
        // With height < 5, no header should be shown
        assert!(!lines[0].contains("ghciwatch"));
    }

    #[test]
    fn medium_terminal_shows_header_but_no_footer() {
        let mut state = make_state(40, 8);
        state.push_line(LineSource::Ghci, "content".into());
        let lines = render_to_lines(&state);
        assert!(lines[0].contains("ghciwatch"));
        // With height < 10, no footer
        assert!(!lines[7].contains("j/k scroll"));
    }

    #[test]
    fn large_terminal_shows_header_and_footer() {
        let mut state = make_state(100, 24);
        state.push_line(LineSource::Ghci, "content".into());
        let lines = render_to_lines(&state);
        assert!(lines[0].contains("ghciwatch"));
        assert!(lines[23].contains("j/k scroll"));
    }

    #[test]
    fn progress_event_stored() {
        let mut state = make_state(40, 10);
        let progress = CompilingProgress {
            module: crate::ghci::parse::CompilingModule {
                name: "Foo".to_string(),
                path: "src/Foo.hs".into(),
            },
            current: 5,
            total: 100,
            reason: None,
        };
        state.handle_tui_event(TuiEvent::CompilationProgress(progress.clone()));
        assert_eq!(state.app.last_progress.as_ref().unwrap().current, 5);
        assert_eq!(state.app.last_progress.as_ref().unwrap().total, 100);
    }

    // --- Phase 4: Tagged lines, file-change, error overlay tests ---

    #[test]
    fn tracing_lines_are_dim_in_visible_text() {
        let mut state = make_state(60, 12);
        state.push_line(LineSource::Ghci, "ghci output".into());
        state.push_line(LineSource::Tracing, "tracing log".into());

        let visible = state.visible_text(10);
        assert!(visible.contains("ghci output"));
        // Tracing lines should be wrapped in ANSI dim codes
        assert!(visible.contains("\x1b[2m"));
        assert!(visible.contains("tracing log"));
    }

    #[test]
    fn file_change_trigger_displayed() {
        let mut state = make_state(80, 12);
        state.handle_tui_event(TuiEvent::CompilationStarted {
            changed_paths: vec!["src/Foo.hs".to_string()],
        });
        assert_eq!(state.app.last_trigger.as_deref(), Some("src/Foo.hs changed"));
    }

    #[test]
    fn multiple_file_changes_show_count() {
        let mut state = make_state(80, 12);
        state.handle_tui_event(TuiEvent::CompilationStarted {
            changed_paths: vec!["a.hs".into(), "b.hs".into(), "c.hs".into()],
        });
        assert_eq!(state.app.last_trigger.as_deref(), Some("3 files changed"));
    }

    #[test]
    fn trigger_appears_in_header() {
        let mut state = make_state(100, 12);
        state.handle_tui_event(TuiEvent::CompilationStarted {
            changed_paths: vec!["src/Bar.hs".into()],
        });
        let lines = render_to_lines(&state);
        assert!(lines[0].contains("src/Bar.hs changed"));
    }

    #[test]
    fn error_overlay_toggles_with_e_key() {
        let mut state = make_state(80, 20);
        state.app.diagnostics = vec![
            make_diagnostic(Severity::Error, "type mismatch"),
        ];
        state.app.error_count = 1;

        state
            .handle_event(key_event(KeyModifiers::NONE, KeyCode::Char('e')))
            .unwrap();
        assert!(state.show_errors);

        state
            .handle_event(key_event(KeyModifiers::NONE, KeyCode::Char('e')))
            .unwrap();
        assert!(!state.show_errors);
    }

    #[test]
    fn error_overlay_closes_with_esc() {
        let mut state = make_state(80, 20);
        state.app.diagnostics = vec![
            make_diagnostic(Severity::Error, "not in scope"),
        ];
        state.app.error_count = 1;
        state.show_errors = true;

        state
            .handle_event(key_event(KeyModifiers::NONE, KeyCode::Esc))
            .unwrap();
        assert!(!state.show_errors);
    }

    #[test]
    fn error_overlay_renders_diagnostics() {
        let mut state = make_state(80, 20);
        state.app.diagnostics = vec![
            make_diagnostic(Severity::Error, "type mismatch"),
            make_diagnostic(Severity::Warning, "unused import"),
        ];
        state.app.error_count = 1;
        state.app.warning_count = 1;
        state.show_errors = true;

        let lines = render_to_lines(&state);
        let content = lines.join("\n");
        assert!(content.contains("type mismatch"));
        assert!(content.contains("unused import"));
        assert!(content.contains("1 errors"));
        assert!(content.contains("1 warnings"));
    }

    #[test]
    fn e_key_does_nothing_without_diagnostics() {
        let mut state = make_state(80, 20);
        state
            .handle_event(key_event(KeyModifiers::NONE, KeyCode::Char('e')))
            .unwrap();
        assert!(!state.show_errors);
    }

    // --- Phase 5: Search, help, scroll indicator tests ---

    #[test]
    fn slash_enters_search_mode() {
        let mut state = make_state(80, 20);
        state
            .handle_event(key_event(KeyModifiers::NONE, KeyCode::Char('/')))
            .unwrap();
        assert!(state.search_mode);
        assert!(state.search_query.is_empty());
    }

    #[test]
    fn search_mode_captures_text() {
        let mut state = make_state(80, 20);
        state.search_mode = true;
        state.handle_event(key_event(KeyModifiers::NONE, KeyCode::Char('f'))).unwrap();
        state.handle_event(key_event(KeyModifiers::NONE, KeyCode::Char('o'))).unwrap();
        state.handle_event(key_event(KeyModifiers::NONE, KeyCode::Char('o'))).unwrap();
        assert_eq!(state.search_query, "foo");
    }

    #[test]
    fn search_mode_backspace_deletes() {
        let mut state = make_state(80, 20);
        state.search_mode = true;
        state.search_query = "foo".into();
        state.handle_event(key_event(KeyModifiers::NONE, KeyCode::Backspace)).unwrap();
        assert_eq!(state.search_query, "fo");
    }

    #[test]
    fn search_mode_esc_cancels() {
        let mut state = make_state(80, 20);
        state.search_mode = true;
        state.search_query = "test".into();
        state.handle_event(key_event(KeyModifiers::NONE, KeyCode::Esc)).unwrap();
        assert!(!state.search_mode);
    }

    #[test]
    fn search_enter_finds_match() {
        let mut state = make_state(80, 20);
        for i in 0..100 {
            state.push_line(LineSource::Ghci, format!("line {i}"));
        }
        state.scroll_to(0);

        state.search_mode = true;
        state.search_query = "line 50".into();
        state.handle_event(key_event(KeyModifiers::NONE, KeyCode::Enter)).unwrap();

        assert!(!state.search_mode);
        assert!(!state.search_matches.is_empty());
        assert_eq!(state.scroll_offset.0, 50);
    }

    #[test]
    fn search_n_navigates_to_next_match() {
        let mut state = make_state(80, 20);
        state.push_line(LineSource::Ghci, "error: first".into());
        for i in 0..10 {
            state.push_line(LineSource::Ghci, format!("ok {i}"));
        }
        state.push_line(LineSource::Ghci, "error: second".into());
        state.scroll_to(0);

        state.search_query = "error".into();
        state.execute_search();
        assert_eq!(state.search_matches.len(), 2);
        assert_eq!(state.scroll_offset.0, 0);

        state.search_next();
        assert_eq!(state.scroll_offset.0, 11);
    }

    #[test]
    fn search_prev_wraps_around() {
        let mut state = make_state(80, 20);
        state.push_line(LineSource::Ghci, "error A".into());
        state.push_line(LineSource::Ghci, "ok".into());
        state.push_line(LineSource::Ghci, "error B".into());

        state.search_query = "error".into();
        state.execute_search();
        assert_eq!(state.search_matches.len(), 2);
        assert_eq!(state.search_match_idx, Some(0));

        state.search_prev();
        assert_eq!(state.search_match_idx, Some(1));
        assert_eq!(state.scroll_offset.0, 2);
    }

    #[test]
    fn search_no_match() {
        let mut state = make_state(80, 20);
        state.push_line(LineSource::Ghci, "hello".into());
        state.search_query = "xyz".into();
        state.execute_search();
        assert!(state.search_matches.is_empty());
        assert_eq!(state.search_match_idx, None);
    }

    #[test]
    fn search_case_insensitive() {
        let mut state = make_state(80, 20);
        state.push_line(LineSource::Ghci, "Error: TYPE MISMATCH".into());
        state.search_query = "type mismatch".into();
        state.execute_search();
        assert_eq!(state.search_matches.len(), 1);
    }

    #[test]
    fn help_toggles_with_question_mark() {
        let mut state = make_state(80, 20);
        state
            .handle_event(key_event(KeyModifiers::NONE, KeyCode::Char('?')))
            .unwrap();
        assert!(state.show_help);

        state
            .handle_event(key_event(KeyModifiers::NONE, KeyCode::Char('?')))
            .unwrap();
        assert!(!state.show_help);
    }

    #[test]
    fn help_closes_with_esc() {
        let mut state = make_state(80, 20);
        state.show_help = true;
        state
            .handle_event(key_event(KeyModifiers::NONE, KeyCode::Esc))
            .unwrap();
        assert!(!state.show_help);
    }

    #[test]
    fn help_overlay_renders() {
        let mut state = make_state(80, 24);
        state.show_help = true;
        let lines = render_to_lines(&state);
        let content = lines.join("\n");
        assert!(content.contains("Keybindings"));
        assert!(content.contains("j / k"));
        assert!(content.contains("Ctrl+c"));
    }

    #[test]
    fn search_mode_footer_shows_input() {
        let mut state = make_state(100, 12);
        state.search_mode = true;
        state.search_query = "test".into();
        let lines = render_to_lines(&state);
        let footer = &lines[11];
        assert!(footer.contains("/test"));
        assert!(footer.contains("Enter confirm"));
    }

    #[test]
    fn footer_shows_scroll_position() {
        let mut state = make_state(100, 12);
        for i in 0..100 {
            state.push_line(LineSource::Ghci, format!("line {i}"));
        }
        state.scroll_to(0);
        let lines = render_to_lines(&state);
        let footer = &lines[11];
        assert!(footer.contains("/100"));
    }

    #[test]
    fn search_mode_does_not_trigger_normal_keys() {
        let mut state = make_state(80, 20);
        state.search_mode = true;
        state.handle_event(key_event(KeyModifiers::NONE, KeyCode::Char('q'))).unwrap();
        assert!(!state.quit);
        assert_eq!(state.search_query, "q");
    }

    // --- Indentation handling tests ---

    #[test]
    fn cap_indent_preserves_short_indent() {
        assert_eq!(cap_indent("    hello"), "    hello");
        assert_eq!(cap_indent("        world"), "        world");
    }

    #[test]
    fn cap_indent_caps_long_indent() {
        let line = format!("{}content", " ".repeat(60));
        let result = cap_indent(&line);
        let indent = result.len() - result.trim_start().len();
        assert_eq!(indent, MAX_LINE_INDENT);
        assert!(result.ends_with("content"));
    }

    #[test]
    fn cap_indent_no_indent_unchanged() {
        assert_eq!(cap_indent("no indent"), "no indent");
    }

    #[test]
    fn cap_indent_empty_line() {
        assert_eq!(cap_indent(""), "");
    }

    #[test]
    fn dedent_removes_common_indent() {
        let input = "    line one\n        line two\n    line three";
        let result = dedent(input);
        assert_eq!(result, "line one\n    line two\nline three");
    }

    #[test]
    fn dedent_high_common_indent() {
        let spaces40 = " ".repeat(40);
        let spaces44 = " ".repeat(44);
        let input = format!("{spaces40}first\n{spaces44}second\n{spaces40}third");
        let result = dedent(&input);
        assert_eq!(result, "first\n    second\nthird");
    }

    #[test]
    fn dedent_skips_empty_lines() {
        let input = "    hello\n\n    world";
        let result = dedent(input);
        assert_eq!(result, "hello\n\nworld");
    }

    #[test]
    fn dedent_no_common_indent() {
        let input = "no indent\n  some indent";
        let result = dedent(input);
        assert_eq!(result, "no indent\n  some indent");
    }

    #[test]
    fn scrollback_lines_are_capped() {
        let mut state = make_state(80, 20);
        let long_indent = format!("{}deeply nested code", " ".repeat(50));
        state.push_line(LineSource::Ghci, long_indent);

        let (_, stored) = state.scrollback.back().unwrap();
        let indent = stored.len() - stored.trim_start().len();
        assert_eq!(indent, MAX_LINE_INDENT);
        assert!(stored.ends_with("deeply nested code"));
    }
}
