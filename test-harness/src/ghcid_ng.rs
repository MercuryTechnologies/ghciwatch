use std::ffi::OsStr;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use miette::miette;
use miette::Context;
use miette::IntoDiagnostic;
use tokio::process::Child;
use tokio::process::Command;

use crate::tracing_reader::TracingReader;
use crate::Event;
use crate::IntoMatcher;
use crate::Matcher;

/// Where to write `ghcid-ng` logs written by integration tests, relative to the temporary
/// directory created for the test.
pub(crate) const LOG_FILENAME: &str = "ghcid-ng.json";

/// `ghcid-ng` session for integration testing.
///
/// This handles copying a directory of files to a temporary directory, starting a `ghcid-ng`
/// session, and asynchronously reading a stream of log events from its JSON log output.
pub struct GhcidNg {
    /// The current working directory of the `ghcid-ng` session.
    cwd: PathBuf,
    /// The `ghcid-ng` child process.
    #[allow(dead_code)]
    child: Child,
    /// A stream of tracing events from `ghcid-ng`.
    tracing_reader: TracingReader,
}

impl GhcidNg {
    /// Start a new `ghcid-ng` session in a copy of the given path.
    pub async fn new(project_directory: impl AsRef<Path>) -> miette::Result<Self> {
        Self::new_with_args(project_directory, std::iter::empty::<&str>()).await
    }

    /// Start a new `ghcid-ng` session in a copy of the given path.
    ///
    /// Also add the given arguments to the `ghcid-ng` invocation.
    pub async fn new_with_args(
        project_directory: impl AsRef<Path>,
        args: impl IntoIterator<Item = impl AsRef<OsStr>>,
    ) -> miette::Result<Self> {
        crate::internal::ensure_in_custom_test_harness()?;
        let ghc_version = crate::internal::get_ghc_version()?;
        let tempdir = crate::internal::set_tempdir()?;
        write_cabal_config(&tempdir).await?;
        check_ghc_version(&tempdir, &ghc_version).await?;

        let project_directory = project_directory.as_ref();
        tracing::info!("Copying project files");
        fs_extra::copy_items(&[project_directory], &tempdir, &Default::default())
            .into_diagnostic()
            .wrap_err("Failed to copy project files")?;

        let project_directory_name = project_directory
            .file_name()
            .ok_or_else(|| miette!("Path has no directory name: {project_directory:?}"))?;

        let cwd = tempdir.join(project_directory_name);

        let log_path = tempdir.join(LOG_FILENAME);

        tracing::info!("Starting ghcid-ng");
        let child = Command::new(test_bin::get_test_bin("ghcid-ng").get_program())
            .arg("--log-json")
            .arg(&log_path)
            .args([
                "--command",
                &format!("cabal --offline v2-repl --with-compiler ghc-{ghc_version}"),
                "--tracing-filter",
                "ghcid_ng=debug",
                "--trace-spans",
                "new,close",
            ])
            .args(args)
            .current_dir(&cwd)
            .env("HOME", &tempdir)
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .into_diagnostic()
            .wrap_err("Failed to start `ghcid-ng`")?;

        // Wait for `ghcid-ng` to create the `log_path`
        tokio::time::timeout(Duration::from_secs(10), crate::fs::wait_for_path(&log_path))
            .await
            .into_diagnostic()
            .wrap_err_with(|| {
                format!("`ghcid-ng` didn't create log path {log_path:?} fast enough")
            })?;

        let tracing_reader = TracingReader::new(log_path.clone()).await?;

        Ok(Self {
            cwd,
            child,
            tracing_reader,
        })
    }

    /// Wait until a matching log event is found.
    ///
    /// Errors if waiting for the event takes longer than the given `timeout`.
    pub async fn get_log_with_timeout(
        &mut self,
        matcher: impl IntoMatcher,
        timeout_duration: Duration,
    ) -> miette::Result<Event> {
        let matcher = matcher.into_matcher()?;

        match tokio::time::timeout(timeout_duration, async {
            loop {
                match self.tracing_reader.next_event().await {
                    Err(err) => {
                        return Err(err);
                    }
                    Ok(event) => {
                        println!("{event}");
                        if matcher.matches(&event) {
                            return Ok(event);
                        }
                    }
                }
            }
        })
        .await
        {
            Ok(Ok(event)) => Ok(event),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(miette!("Waiting for a log message timed out")),
        }
    }

    /// Wait until a matching log event is found, with a default 1-minute timeout.
    pub async fn get_log(&mut self, matcher: impl IntoMatcher) -> miette::Result<Event> {
        self.get_log_with_timeout(matcher, Duration::from_secs(60))
            .await
    }

    /// Get a path relative to the project root.
    pub fn path(&self, path: impl AsRef<Path>) -> PathBuf {
        self.cwd.join(path)
    }
}

/// Write an empty `~/.cabal/config` so that `cabal` doesn't try to access the internet.
///
/// See: <https://github.com/haskell/cabal/issues/6167>
async fn write_cabal_config(home: &Path) -> miette::Result<()> {
    std::fs::create_dir_all(home.join(".cabal"))
        .into_diagnostic()
        .wrap_err("Failed to create `.cabal` directory")?;
    crate::fs::touch(home.join(".cabal/config"))
        .await
        .wrap_err("Failed to write empty `.cabal/config`")?;
    Ok(())
}

/// Check that `ghc-{ghc_version} --version` executes successfully.
///
/// This is a nice check that the given GHC version is present in the environment, to fail tests
/// early without waiting for `ghcid-ng` to fail.
async fn check_ghc_version(home: &Path, ghc_version: &str) -> miette::Result<()> {
    let _output = Command::new(format!("ghc-{ghc_version}"))
        .env("HOME", home)
        .output()
        .await
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to find GHC {ghc_version}"))?;
    // `ghc --version` returns a nonzero status code. As long as we could actually execute it, it's
    // OK if it failed.
    Ok(())
}
