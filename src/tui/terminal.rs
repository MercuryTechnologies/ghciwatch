use crossterm::cursor;
use crossterm::event;
use crossterm::terminal;
use miette::miette;
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
use tracing::instrument;

/// A wrapper around a [`Terminal`] that disables the terminal's raw mode when it's dropped.
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
        if let Err(error) = exit().wrap_err("Failed to exit terminal during drop") {
            if std::thread::panicking() {
                // Ignore the `Result` if we're already panicking; aborting is undesirable.
                tracing::error!("{error}");
            } else {
                panic!("{error}");
            }
        }
    }
}

/// Are we currently inside a raw-mode terminal?
///
/// This helps us avoid entering or exiting raw-mode twice.
static INSIDE: AtomicBool = AtomicBool::new(false);

/// Enter raw-mode for the terminal on stdout, set up a panic hook, etc.
#[instrument(level = "debug")]
pub fn enter() -> miette::Result<TerminalGuard> {
    use event::KeyboardEnhancementFlags as KEF;

    if INSIDE.load(Ordering::SeqCst) {
        return Err(miette!(
            "Cannot enter raw mode; the terminal is already set up"
        ));
    }

    // Set `INSIDE` immediately so that a partial load is rolled back by `exit()`.
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

/// Exits terminal raw-mode.
#[instrument(level = "debug")]
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
