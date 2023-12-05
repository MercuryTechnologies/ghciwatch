use crate::ghci::parse::CompilationResult;
use crate::ghci::parse::CompilationSummary;
use crate::ghci::parse::GhcDiagnostic;
use crate::ghci::parse::GhcMessage;
use crate::ghci::parse::Severity;

/// A log of messages from compilation, used to write the error log.
#[derive(Debug, Clone, Default)]
pub struct CompilationLog {
    pub summary: Option<CompilationSummary>,
    pub diagnostics: Vec<GhcDiagnostic>,
}

impl CompilationLog {
    /// Get the result of compilation.
    pub fn result(&self) -> Option<CompilationResult> {
        self.summary.map(|summary| summary.result)
    }
}

impl Extend<GhcMessage> for CompilationLog {
    fn extend<T: IntoIterator<Item = GhcMessage>>(&mut self, iter: T) {
        for message in iter {
            match message {
                GhcMessage::Compiling(module) => {
                    tracing::debug!(module = %module.name, path = %module.path, "Compiling");
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
