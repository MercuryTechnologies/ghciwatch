use camino::Utf8PathBuf;
use miette::IntoDiagnostic;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::io::BufWriter;
use tracing::instrument;

use super::parse::CompilationResult;
use super::parse::ModulesLoaded;
use super::CompilationLog;

/// Error log writer.
///
/// This produces `ghcid`-compatible output, which can be consumed by `ghcid` plugins in your
/// editor of choice.
pub struct ErrorLog {
    path: Option<Utf8PathBuf>,
}

impl ErrorLog {
    /// Construct a new error log writer for the given path.
    pub fn new(path: Option<Utf8PathBuf>) -> Self {
        Self { path }
    }

    /// Write the error log, if any, with the given compilation summary and diagnostic messages.
    #[instrument(skip(self, log), name = "error_log_write", level = "debug")]
    pub async fn write(&mut self, log: &CompilationLog) -> miette::Result<()> {
        let path = match &self.path {
            Some(path) => path,
            None => {
                tracing::debug!("No error log path, not writing");
                return Ok(());
            }
        };

        let file = File::create(path).await.into_diagnostic()?;
        let mut writer = BufWriter::new(file);

        if let Some(summary) = log.summary {
            // `ghcid` only writes the headline if there's no errors.
            if let CompilationResult::Ok = summary.result {
                tracing::debug!(%path, "Writing 'All good'");
                let modules_loaded = if summary.modules_loaded != ModulesLoaded::Count(1) {
                    format!("{} modules", summary.modules_loaded)
                } else {
                    format!("{} module", summary.modules_loaded)
                };
                writer
                    .write_all(format!("All good ({modules_loaded})\n").as_bytes())
                    .await
                    .into_diagnostic()?;
            }
        }

        for diagnostic in &log.diagnostics {
            tracing::debug!(%diagnostic, "Writing diagnostic");
            writer
                .write_all(diagnostic.to_string().as_bytes())
                .await
                .into_diagnostic()?;
        }

        // This is load-bearing! If we don't properly flush/shutdown the handle, nothing gets
        // written!
        writer.shutdown().await.into_diagnostic()?;

        Ok(())
    }
}
