use crate::ghci::parse::CompilationSummary;
use crate::ghci::parse::CompilingProgress;
use crate::ghci::parse::GhcDiagnostic;
use crate::hooks::LifecycleEvent;

/// Structured events sent from the GHCi task to the TUI for rendering.
#[derive(Debug, Clone)]
pub enum TuiEvent {
    /// Compilation has started (e.g. after a file change triggered a reload).
    CompilationStarted {
        /// Paths that changed and triggered this compilation, if known.
        changed_paths: Vec<String>,
    },
    /// A module is being compiled; carries progress info like `[N of M]`.
    CompilationProgress(CompilingProgress),
    /// Compilation finished; carries the summary and diagnostics.
    CompilationFinished {
        /// The compilation summary (ok/err, module count).
        summary: CompilationSummary,
        /// Diagnostics (errors and warnings) from the compilation.
        diagnostics: Vec<GhcDiagnostic>,
    },
    /// A lifecycle event (startup, reload, restart, test).
    Lifecycle(LifecycleEvent),
    /// The TUI should insert a visual separator (e.g. on `--clear`).
    Clear,
}
