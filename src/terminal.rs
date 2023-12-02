use crossterm::{cursor, event, terminal};
use miette::{IntoDiagnostic as _, WrapErr as _};
use ratatui::prelude::{CrosstermBackend, Terminal};
use std::{
    io::Stdout,
    ops::{Deref, DerefMut},
    panic,
};

pub struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl Deref for TerminalGuard {
    type Target = Terminal<CrosstermBackend<Stdout>>;

    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}

impl DerefMut for TerminalGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.terminal
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        exit()
            .wrap_err("Failed to exit terminal during drop")
            .unwrap();
    }
}

pub fn enter() -> miette::Result<TerminalGuard> {
    use event::KeyboardEnhancementFlags as KEF;

    let mut stdout = std::io::stdout();

    terminal::enable_raw_mode()
        .into_diagnostic()
        .wrap_err("Failed to enable raw mode")?;

    crossterm::execute!(
        stdout,
        terminal::EnterAlternateScreen,
        cursor::Hide,
        event::EnableMouseCapture,
        event::EnableFocusChange,
        event::EnableBracketedPaste,
        event::PushKeyboardEnhancementFlags(
            KEF::DISAMBIGUATE_ESCAPE_CODES
                | KEF::REPORT_EVENT_TYPES
                | KEF::REPORT_ALL_KEYS_AS_ESCAPE_CODES
        ),
    )
    .into_diagnostic()
    .wrap_err("Failed to execute crossterm commands")?;

    let previous_hook = panic::take_hook();

    panic::set_hook(Box::new(move |panic_info| {
        exit()
            .wrap_err("Failed to exit terminal during panic")
            .unwrap();
        previous_hook(panic_info);
    }));

    let backend = CrosstermBackend::new(stdout);

    let terminal = Terminal::new(backend)
        .into_diagnostic()
        .wrap_err("Failed to create ratatui terminal")?;

    Ok(TerminalGuard { terminal })
}

pub fn exit() -> miette::Result<()> {
    let mut stdout = std::io::stdout();

    crossterm::execute!(
        stdout,
        event::PopKeyboardEnhancementFlags,
        event::DisableBracketedPaste,
        event::DisableFocusChange,
        event::DisableMouseCapture,
        cursor::Show,
        terminal::LeaveAlternateScreen,
    )
    .into_diagnostic()
    .wrap_err("Failed to execute crossterm commands")?;

    terminal::disable_raw_mode()
        .into_diagnostic()
        .wrap_err("Failed to disable raw mode")?;

    if !std::thread::panicking() {
        let _ = panic::take_hook();
    }

    Ok(())
}
