use std::ffi::OsStr;
use std::ffi::OsString;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use futures_util::future::BoxFuture;
use miette::miette;
use miette::Context;
use miette::IntoDiagnostic;
use tokio::process::Command;

use crate::tracing_reader::TracingReader;
use crate::Event;
use crate::GhcVersion;
use crate::IntoMatcher;
use crate::Matcher;

/// Where to write `ghcid-ng` logs written by integration tests, relative to the temporary
/// directory created for the test.
pub(crate) const LOG_FILENAME: &str = "ghcid-ng.json";

/// Builder for [`GhcidNg`].
pub struct GhcidNgBuilder {
    project_directory: PathBuf,
    args: Vec<OsString>,
    #[allow(clippy::type_complexity)]
    before_start: Option<Box<dyn FnOnce(PathBuf) -> BoxFuture<'static, miette::Result<()>> + Send>>,
}

impl GhcidNgBuilder {
    /// Create a new builder for a `ghcid-ng` session with the given project directory.
    pub fn new(project_directory: impl AsRef<Path>) -> Self {
        Self {
            project_directory: project_directory.as_ref().to_owned(),
            args: Default::default(),
            before_start: None,
        }
    }

    /// Add an argument to the `ghcid-ng` invocation.
    pub fn with_arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.args.push(arg.as_ref().to_owned());
        self
    }

    /// Add multiple arguments to the `ghcid-ng` invocation.
    pub fn with_args(mut self, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Self {
        self.args
            .extend(args.into_iter().map(|s| s.as_ref().to_owned()));
        self
    }

    /// Add a hook to run after project files are copied to the temporary directory but before
    /// `ghcid-ng` is started.
    pub fn before_start<F>(mut self, before_start: impl Fn(PathBuf) -> F + Send + 'static) -> Self
    where
        F: Future<Output = miette::Result<()>> + Send + 'static,
    {
        self.before_start = Some(Box::new(move |path| Box::pin(before_start(path))));
        self
    }

    /// Start `ghcid-ng`.
    pub async fn start(self) -> miette::Result<GhcidNg> {
        GhcidNg::from_builder(self).await
    }
}

/// `ghcid-ng` session for integration testing.
///
/// This handles copying a directory of files to a temporary directory, starting a `ghcid-ng`
/// session, and asynchronously reading a stream of log events from its JSON log output.
pub struct GhcidNg {
    /// The current working directory of the `ghcid-ng` session.
    cwd: PathBuf,
    /// A stream of tracing events from `ghcid-ng`.
    tracing_reader: TracingReader,
    /// The major version of GHC this test is running under.
    ghc_version: GhcVersion,
}

impl GhcidNg {
    async fn from_builder(mut builder: GhcidNgBuilder) -> miette::Result<Self> {
        let full_ghc_version = crate::internal::get_ghc_version()?;
        let ghc_version = full_ghc_version.parse()?;
        let tempdir = crate::internal::set_tempdir()?;
        write_cabal_config(&tempdir).await?;
        check_ghc_version(&tempdir, &full_ghc_version).await?;

        tracing::info!("Copying project files");
        fs_extra::copy_items(&[&builder.project_directory], &tempdir, &Default::default())
            .into_diagnostic()
            .wrap_err("Failed to copy project files")?;

        let project_directory_name = builder.project_directory.file_name().ok_or_else(|| {
            miette!(
                "Path has no directory name: {:?}",
                builder.project_directory
            )
        })?;

        let cwd = tempdir.join(project_directory_name);

        if let Some(before_start) = builder.before_start.take() {
            let future = (before_start)(cwd.clone());
            future.await?;
        }

        let log_path = tempdir.join(LOG_FILENAME);

        tracing::info!("Starting ghcid-ng");
        let mut command = Command::new(test_bin::get_test_bin("ghcid-ng").get_program());
        command
            .arg("--log-json")
            .arg(&log_path)
            .args([
                "--command",
                &format!(
                    "cabal --offline --with-compiler=ghc-{full_ghc_version} -flocal-dev --repl-option -fdiagnostics-color=always v2-repl lib:test-dev"
                ),
                "--before-startup-shell",
                "hpack --force .",
                "--tracing-filter",
                &[
                    "ghcid_ng::watcher=trace",
                    "ghcid_ng=debug",
                    "watchexec=debug",
                    "watchexec::fs=trace",
                ].join(","),
                "--trace-spans",
                "new,close",
                "--poll",
                "1000ms",
            ])
            .args(builder.args)
            .current_dir(&cwd)
            .env("HOME", &tempdir)
            // GHC will quote things with Unicode quotes unless we set this variable.
            // Very cute.
            // https://gitlab.haskell.org/ghc/ghc/-/blob/288235bbe5a59b8a1bda80aaacd59e5717417726/compiler/GHC/Driver/Session.hs#L1084-L1085
            // https://gitlab.haskell.org/ghc/ghc/-/blob/288235bbe5a59b8a1bda80aaacd59e5717417726/compiler/GHC/Utils/Outputable.hs#L728-L740
            .env("GHC_NO_UNICODE", "1")
            .stderr(Stdio::inherit())
            .stdout(Stdio::inherit())
            .kill_on_drop(true);

        let mut child = command
            .spawn()
            .into_diagnostic()
            .wrap_err("Failed to start `ghcid-ng`")?;

        // Wait for `ghcid-ng` to create the `log_path`
        let creates_log_path =
            tokio::time::timeout(Duration::from_secs(10), crate::fs::wait_for_path(&log_path));
        tokio::select! {
            child_result = child.wait() => {
                return match child_result {
                    Err(err) => {
                        Err(err).into_diagnostic().wrap_err("ghcid-ng failed to execute")
                    }
                    Ok(status) => {
                        Err(miette!("ghcid-ng exited: {status}"))
                    }
                }
            }
            log_path_result = creates_log_path => {
                if log_path_result.is_err() {
                    return Err(miette!("`ghcid-ng` didn't create log path {log_path:?} fast enough"));
                }
            }
            else => {}
        }

        crate::internal::set_ghc_process(child)?;

        let tracing_reader = TracingReader::new(log_path.clone()).await?;

        Ok(Self {
            cwd,
            tracing_reader,
            ghc_version,
        })
    }

    /// Start a new `ghcid-ng` session in a copy of the given path.
    pub async fn new(project_directory: impl AsRef<Path>) -> miette::Result<Self> {
        GhcidNgBuilder::new(project_directory).start().await
    }

    /// Wait until a matching log event is found.
    ///
    /// If `negative_matcher` is given, no log events may match it until an event matching the
    /// regular `matcher` is found.
    ///
    /// Errors if waiting for the event takes longer than the given `timeout`.
    pub async fn assert_logged_with_timeout(
        &mut self,
        negative_matcher: Option<Matcher>,
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
                        if matcher.matches(&event) {
                            return Ok(event);
                        } else if let Some(negative_matcher) = &negative_matcher {
                            if negative_matcher.matches(&event) {
                                return Err(miette!("Found a log event matching {negative_matcher}"));
                            }
                        }
                    }
                }
            }
        })
        .await
        {
            Ok(Ok(event)) => Ok(event),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(miette!(
                "Waiting for a log message matching {matcher} timed out after {timeout_duration:.2?}"
            )),
        }
    }

    /// Wait until a matching log event is found, with a default 10-second timeout.
    pub async fn assert_logged(&mut self, matcher: impl IntoMatcher) -> miette::Result<Event> {
        self.assert_logged_with_timeout(None, matcher, Duration::from_secs(10))
            .await
    }

    /// Wait until a matching log event is found, with a default 10-second timeout.
    ///
    /// Error if a log event matching `negative_matcher` is found before an event matching
    /// `matcher`.
    pub async fn assert_not_logged(
        &mut self,
        negative_matcher: impl IntoMatcher,
        matcher: impl IntoMatcher,
    ) -> miette::Result<Event> {
        self.assert_logged_with_timeout(
            Some(negative_matcher.into_matcher()?),
            matcher,
            Duration::from_secs(10),
        )
        .await
    }

    /// Wait until `ghcid-ng` completes its initial load.
    pub async fn wait_until_started(&mut self) -> miette::Result<Event> {
        self.assert_logged_with_timeout(
            None,
            r"ghci started in \d+\.\d+m?s",
            Duration::from_secs(60),
        )
        .await
        .wrap_err("ghcid-ng didn't start in time")
    }

    /// Wait until `ghcid-ng` is ready to receive file events.
    pub async fn wait_until_watcher_started(&mut self) -> miette::Result<Event> {
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
        self.assert_logged(
            Matcher::message("applying changes to the watcher").in_module("watchexec::fs"),
        )
        .await
        .wrap_err("watchexec filesystem worker didn't start in time")
    }

    /// Wait until `ghcid-ng` completes its initial load and is ready to receive file events.
    pub async fn wait_until_ready(&mut self) -> miette::Result<()> {
        self.wait_until_started().await?;
        self.wait_until_watcher_started().await?;
        Ok(())
    }

    /// Wait until `ghcid-ng` reloads the `ghci` session due to changed modules.
    pub async fn wait_until_reload(&mut self) -> miette::Result<()> {
        // TODO: It would be nice to verify which modules are changed.
        self.assert_logged("Reloading ghci due to changed modules")
            .await
            .map(|_| ())
    }

    /// Wait until `ghcid-ng` adds new modules to the `ghci` session.
    pub async fn wait_until_add(&mut self) -> miette::Result<()> {
        // TODO: It would be nice to verify which modules are being added.
        self.assert_logged("Adding new modules to ghci")
            .await
            .map(|_| ())
    }

    /// Wait until `ghcid-ng` restarts the `ghci` session.
    pub async fn wait_until_restart(&mut self) -> miette::Result<()> {
        // TODO: It would be nice to verify which modules have been deleted/moved.
        self.assert_logged("Restarting ghci due to deleted/moved modules")
            .await
            .map(|_| ())
    }

    /// Get a path relative to the project root.
    pub fn path(&self, path: impl AsRef<Path>) -> PathBuf {
        self.cwd.join(path)
    }

    /// Get the major GHC version this test is running under.
    pub fn ghc_version(&self) -> GhcVersion {
        self.ghc_version
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
