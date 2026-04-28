use camino::Utf8Path;

use crate::ghci::parse::CompilationResult;
use crate::ghci::parse::CompilationSummary;
use crate::ghci::parse::GhcDiagnostic;
use crate::ghci::parse::GhcMessage;
use crate::ghci::parse::Severity;

use super::parse::ModulesLoaded;

/// A log of messages from compilation, used to write the error log.
#[derive(Debug, Clone, Default)]
pub struct CompilationLog {
    pub summary: Option<CompilationSummary>,
    pub diagnostics: Vec<GhcDiagnostic>,
}

impl CompilationLog {
    /// Make the diagnostic paths for this log relative to a different directory.
    pub fn relocate(&mut self, old_base: &Utf8Path, new_base: &Utf8Path) -> eyre::Result<()> {
        for diagnostic in self.diagnostics.iter_mut() {
            diagnostic.make_relative_to(old_base, new_base)?;
        }
        Ok(())
    }

    /// If we start up in `--repl-no-load`, we don't get a compilation summary, but we don't want to
    /// leave the error log empty, so we synthesize an "All good (0 modules)" message.
    pub fn fill_empty_summary(&mut self) {
        self.summary.get_or_insert_with(|| {
            // We usually infer the success status from the "Ok" or "Failed" message at the end of
            // compilation. If we don't have that, let's use the presence of error diagnostics as a
            // proxy.

            let has_errors = self
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == Severity::Error);

            let result = if has_errors {
                CompilationResult::Err
            } else {
                CompilationResult::Ok
            };

            CompilationSummary {
                result,
                modules_loaded: ModulesLoaded::Count(0),
            }
        });
    }

    /// Get the result of compilation.
    pub fn result(&self) -> Option<CompilationResult> {
        self.summary.map(|summary| summary.result)
    }
}

impl Extend<GhcMessage> for CompilationLog {
    fn extend<T: IntoIterator<Item = GhcMessage>>(&mut self, iter: T) {
        for message in iter {
            match message {
                GhcMessage::Compiling(progress) => {
                    tracing::debug!(
                        module = %progress.module.name,
                        path = %progress.module.path,
                        current = progress.current,
                        total = progress.total,
                        reason = progress.reason.as_deref().unwrap_or(""),
                        "Compiling",
                    );
                }
                GhcMessage::Diagnostic(diagnostic) => {
                    if let GhcDiagnostic {
                        severity: Severity::Error,
                        path: Some(path),
                        message,
                        ..
                    } = &diagnostic
                    {
                        // We can't use 'message' for the field name because that's what tracing uses
                        // for the message.
                        tracing::debug!(%path, error = message, "Module failed to compile");
                    }
                    self.diagnostics.push(diagnostic);
                }
                GhcMessage::Summary(summary) => {
                    self.summary = Some(summary);
                    match summary.result {
                        CompilationResult::Ok => {
                            tracing::debug!("Compilation succeeded");
                        }
                        CompilationResult::Err => {
                            tracing::debug!("Compilation failed");
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
