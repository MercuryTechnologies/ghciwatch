use std::ffi::OsStr;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use miette::miette;
use miette::Context;
use miette::IntoDiagnostic;
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
        let mut command = Command::new(test_bin::get_test_bin("ghcid-ng").get_program());
        command
            .arg("--log-json")
            .arg(&log_path)
            .args([
                "--command",
                &format!("cabal --offline v2-repl --with-compiler ghc-{ghc_version}"),
                "--tracing-filter",
                "ghcid_ng::watcher=trace,ghcid_ng=debug,watchexec=debug,watchexec::fs=trace",
                "--trace-spans",
                "new,close",
            ])
            .args(args)
            .current_dir(&cwd)
            .env("HOME", &tempdir)
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true);

        #[cfg(target_os = "macos")]
        command.args(["--poll", "1000ms"]);

        let child = command
            .spawn()
            .into_diagnostic()
            .wrap_err("Failed to start `ghcid-ng`")?;

        crate::internal::set_ghc_process(child)?;

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

    /// Wait until a matching log event is found, with a default 10-second timeout.
    pub async fn get_log(&mut self, matcher: impl IntoMatcher) -> miette::Result<Event> {
        self.get_log_with_timeout(matcher, Duration::from_secs(10))
            .await
    }

    /// Wait until `ghcid-ng` completes its initial load and is ready to receive file events.
    pub async fn wait_until_ready(&mut self) -> miette::Result<()> {
        self.get_log(r"ghci started in \d+\.\d+m?s")
            .await
            .wrap_err("ghcid-ng didn't start in time")?;
        // Only _after_ `ghci` starts up do we initialize the file watcher.
        // `watchexec` sends a few events when it starts up:
        //
        // DEBUG watchexec::watchexec: handing over main task handle
        // DEBUG watchexec::watchexec: starting main task
        // DEBUG watchexec::watchexec: spawning subtask {subtask="action"}
        // DEBUG watchexec::watchexec: spawning subtask {subtask="fs"}
        // DEBUG watchexec::watchexec: spawning subtask {subtask="signal"}
        // DEBUG watchexec::watchexec: spawning subtask {subtask="keyboard"}
        // DEBUG watchexec::fs: launching filesystem worker
        // DEBUG watchexec::watchexec: spawning subtask {subtask="error_hook"}
        // DEBUG watchexec::fs: creating new watcher {kind="Poll(100ms)"}
        // DEBUG watchexec::signal: launching unix signal worker
        // DEBUG watchexec::fs: applying changes to the watcher {to_drop="[]", to_watch="[WatchedPath(\"src\")]"}
        //
        // "launching filesystem worker" is tempting, but the phrasing implies the event is emitted
        // _before_ the filesystem worker is started (hence it is not yet ready to notice file
        // events). Therefore, we wait for "applying changes to the watcher".
        self.get_log(
            Matcher::message("applying changes to the watcher")
                .expect("Compiling the regex will not fail")
                .in_module("watchexec::fs"),
        )
        .await
        .wrap_err("watchexec filesystem worker didn't start in time")?;
        Ok(())
    }

    /// Wait until `ghcid-ng` reloads the `ghci` session due to changed modules.
    pub async fn wait_until_reload(&mut self) -> miette::Result<()> {
        // TODO: It would be nice to verify which modules are changed.
        self.get_log("Reloading ghci due to changed modules")
            .await
            .map(|_| ())
    }

    /// Wait until `ghcid-ng` adds new modules to the `ghci` session.
    pub async fn wait_until_add(&mut self) -> miette::Result<()> {
        // TODO: It would be nice to verify which modules are being added.
        self.get_log("Adding new modules to ghci").await.map(|_| ())
    }

    /// Wait until `ghcid-ng` restarts the `ghci` session.
    pub async fn wait_until_restart(&mut self) -> miette::Result<()> {
        // TODO: It would be nice to verify which modules have been deleted/moved.
        self.get_log("Restarting ghci due to deleted/moved modules")
            .await
            .map(|_| ())
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
