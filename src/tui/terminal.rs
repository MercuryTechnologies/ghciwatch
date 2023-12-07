use crossterm::cursor;
use crossterm::event;
use crossterm::terminal;
use miette::IntoDiagnostic;
use miette::WrapErr;
use ratatui::prelude::CrosstermBackend;
use ratatui::prelude::Terminal;
use std::io::Stdout;
use std::ops::Deref;
use std::ops::DerefMut;
use std::panic;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

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
        let result = exit().wrap_err("Failed to exit terminal during drop");
        // Ignore the `Result` if we're already panicking; aborting is undesirable
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}

static INSIDE: AtomicBool = AtomicBool::new(false);

pub fn enter() -> miette::Result<TerminalGuard> {
    use event::KeyboardEnhancementFlags as KEF;

    INSIDE.store(true, Ordering::SeqCst);

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
        // Ignoring the `Result` because we're already panicking; aborting is undesirable
        let _ = exit();
        previous_hook(panic_info);
    }));

    let backend = CrosstermBackend::new(stdout);

    let terminal = Terminal::new(backend)
        .into_diagnostic()
        .wrap_err("Failed to create ratatui terminal")?;

    Ok(TerminalGuard { terminal })
}

pub fn exit() -> miette::Result<()> {
    if !INSIDE.load(Ordering::SeqCst) {
        return Ok(());
    }

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

    INSIDE.store(false, Ordering::SeqCst);

    Ok(())
}
