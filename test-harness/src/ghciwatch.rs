use std::ffi::OsStr;
use std::ffi::OsString;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command as StdCommand;
use std::process::ExitStatus;
use std::time::Duration;

use clonable_command::Command as ClonableCommand;
use eyre::eyre;
use eyre::Context;
use futures_util::future::BoxFuture;
use itertools::Itertools;
use tap::Conv;
use tokio::process::Command;

use crate::matcher::Matcher;
use crate::timeout_mult::timeout_mult;
use crate::tracing_reader::TracingReader;
use crate::BaseMatcher;
use crate::Event;
use crate::Fs;
use crate::FullGhcVersion;
use crate::GhcVersion;
use crate::IntoMatcher;

/// Where to write `ghciwatch` logs written by integration tests, relative to the temporary
/// directory created for the test.
pub(crate) const LOG_FILENAME: &str = "ghciwatch.json";

/// Builder for [`GhciWatch`].
pub struct GhciWatchBuilder {
    project_directory: PathBuf,
    ghciwatch_args: Vec<OsString>,
    ghc_args: Vec<String>,
    cabal_args: Vec<String>,
    cabal_target: String,
    #[allow(clippy::type_complexity)]
    before_start: Option<Box<dyn FnOnce(PathBuf) -> BoxFuture<'static, eyre::Result<()>> + Send>>,
    default_timeout: Duration,
    startup_timeout: Duration,
    log_filters: Vec<String>,
    log_filters_json: Vec<String>,
}

impl GhciWatchBuilder {
    /// Create a new builder for a `ghciwatch` session with the given project directory.
    pub fn new(project_directory: impl AsRef<Path>) -> Self {
        Self {
            project_directory: project_directory.as_ref().to_owned(),
            ghciwatch_args: Default::default(),
            ghc_args: Default::default(),
            cabal_args: Default::default(),
            cabal_target: "my-simple-package".into(),
            before_start: None,
            // Note: These will be scaled by `timeout_mult` later.
            default_timeout: Duration::from_secs(7),
            startup_timeout: Duration::from_secs(10),
            log_filters: Default::default(),
            log_filters_json: Default::default(),
        }
    }

    /// Add an argument to the `ghciwatch` invocation.
    pub fn with_arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.ghciwatch_args.push(arg.as_ref().to_owned());
        self
    }

    /// Add multiple arguments to the `ghciwatch` invocation.
    pub fn with_args(mut self, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Self {
        self.ghciwatch_args
            .extend(args.into_iter().map(|s| s.as_ref().to_owned()));
        self
    }

    /// Add a GHC argument to the `cabal repl` invocation.
    pub fn with_ghc_arg(mut self, arg: impl AsRef<str>) -> Self {
        self.ghc_args.push(arg.as_ref().to_owned());
        self
    }

    /// Add multiple GHC arguments to the `cabal repl` invocation.
    pub fn with_ghc_args(mut self, args: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.ghc_args
            .extend(args.into_iter().map(|s| s.as_ref().to_owned()));
        self
    }

    /// Add an argument to the `cabal` invocations.
    pub fn with_cabal_arg(mut self, arg: impl AsRef<str>) -> Self {
        self.cabal_args.push(arg.as_ref().to_owned());
        self
    }

    /// Add multiple arguments to the `cabal` invocations.
    pub fn with_cabal_args(mut self, args: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.cabal_args
            .extend(args.into_iter().map(|s| s.as_ref().to_owned()));
        self
    }

    /// Set the Cabal target to open a `cabal v2-repl` for.
    pub fn with_cabal_target(mut self, target: impl AsRef<str>) -> Self {
        self.cabal_target = target.as_ref().to_owned();
        self
    }

    /// Add a hook to run after project files are copied to the temporary directory but before
    /// `ghciwatch` is started.
    pub fn before_start<F>(mut self, before_start: impl Fn(PathBuf) -> F + Send + 'static) -> Self
    where
        F: Future<Output = eyre::Result<()>> + Send + 'static,
    {
        self.before_start = Some(Box::new(move |path| Box::pin(before_start(path))));
        self
    }

    /// Set the default timeout to wait for log messages in [`GhciWatch::wait_for_log`],
    /// [`GhciWatch::assert_logged_or_wait`], and similar.
    ///
    /// This is multiplied with [`timeout_mult`].
    pub fn with_default_timeout(mut self, default_timeout: Duration) -> Self {
        self.default_timeout = default_timeout;
        self
    }

    /// Set the default timeout to wait for `ghci` to start up in
    /// [`GhciWatch::wait_until_started`] and [`GhciWatch::wait_until_ready`].
    ///
    /// This is multiplied with [`timeout_mult`].
    pub fn with_startup_timeout(mut self, startup_timeout: Duration) -> Self {
        self.startup_timeout = startup_timeout;
        self
    }

    /// Start `ghciwatch`.
    pub async fn start(self) -> eyre::Result<GhciWatch> {
        GhciWatch::from_builder(self).await
    }

    /// Add a `--log-filter` clause to the `ghciwatch` invocation.
    pub fn with_log_filter(mut self, log_filter: impl AsRef<str>) -> Self {
        self.log_filters.push(log_filter.as_ref().to_owned());
        self
    }

    /// Add multiple `--log-filter` clauses to the `ghciwatch` invocation.
    pub fn with_log_filters(
        mut self,
        log_filters: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        self.log_filters
            .extend(log_filters.into_iter().map(|s| s.as_ref().to_owned()));
        self
    }

    /// Add a `--log-filter-json` clause to the `ghciwatch` invocation.
    pub fn with_log_filter_json(mut self, log_filter_json: impl AsRef<str>) -> Self {
        self.log_filters_json
            .push(log_filter_json.as_ref().to_owned());
        self
    }

    /// Add multiple `--log-filter-json` clauses to the `ghciwatch` invocation.
    pub fn with_log_filters_json(
        mut self,
        log_filter_jsons: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        self.log_filters_json
            .extend(log_filter_jsons.into_iter().map(|s| s.as_ref().to_owned()));
        self
    }

    fn get_log_filters_inner<const N: usize>(
        env_var: &str,
        default_filters: [&str; N],
        builder_filters: &[String],
    ) -> String {
        let env_var_filters = match std::env::var(env_var) {
            Ok(var) => {
                vec![var]
            }
            Err(std::env::VarError::NotPresent) => {
                vec![]
            }
            Err(err @ std::env::VarError::NotUnicode(_)) => {
                tracing::warn!("${env_var} isn't UTF-8: {err}");
                vec![]
            }
        };

        let mut filters = default_filters
            .into_iter()
            .chain(builder_filters.iter().map(|s| s.as_ref()))
            .chain(env_var_filters.iter().map(|s| s.as_ref()));

        filters.join(",")
    }

    fn get_log_filters(&self) -> String {
        Self::get_log_filters_inner("GHCIWATCH_LOG", ["info"], &self.log_filters)
    }

    fn get_json_log_filters(&self) -> String {
        Self::get_log_filters_inner(
            "GHCIWATCH_LOG_JSON",
            ["ghciwatch=debug"],
            &self.log_filters_json,
        )
    }
}

struct Session {
    /// A stream of tracing events from `ghciwatch`.
    tracing_reader: TracingReader,
    //// The `ghciwatch` process's PID.
    pid: u32,
}

impl Session {
    /// Wait for `ghciwatch` to create the `log_path`
    async fn new(
        command: &ClonableCommand,
        timeout: Duration,
        log_path: &Path,
    ) -> eyre::Result<Self> {
        let fs = Fs::new();
        if log_path.exists() {
            fs.remove(log_path).await?;
        }

        tracing::info!("Starting ghciwatch");
        let mut child = command
            .conv::<StdCommand>()
            .conv::<Command>()
            .kill_on_drop(true)
            .spawn()
            .wrap_err("Failed to start `ghciwatch`")?;

        let creates_log_path = fs.wait_for_path(timeout, log_path);
        tokio::select! {
            child_result = child.wait() => {
                return match child_result {
                    Err(err) => {
                        Err(err).wrap_err("ghciwatch failed to execute")
                    }
                    Ok(status) => {
                        Err(eyre!("ghciwatch exited: {status}"))
                    }
                }
            }
            log_path_result = creates_log_path => {
                log_path_result?;
            }
            else => {}
        }

        let pid = child.id().ok_or_else(|| eyre!("`ghciwatch` has no PID"))?;

        crate::internal::set_ghciwatch_process(child)?;

        let tracing_reader = TracingReader::new(&log_path).await?;

        Ok(Self {
            tracing_reader,
            pid,
        })
    }
}

/// `ghciwatch` session for integration testing.
///
/// This handles copying a directory of files to a temporary directory, starting a `ghciwatch`
/// session, and asynchronously reading a stream of log events from its JSON log output.
pub struct GhciWatch {
    /// The command which started the `ghciwatch` session.
    command: clonable_command::Command,
    /// Path to the log file where `ghciwatch` writes events.
    log_path: PathBuf,
    /// The current working directory of the `ghciwatch` session.
    cwd: PathBuf,
    /// All logged events read so far.
    events: Vec<Event>,
    /// The version of GHC this test is running under.
    ghc_version: FullGhcVersion,
    /// The default timeout for waiting for log messages.
    default_timeout: Duration,
    /// The timeout for waiting for `ghci to finish starting up.
    pub startup_timeout: Duration,
    /// Filesystem helpers.
    fs: Fs,
    /// Data for this particular `ghciwatch` run. This changes when
    /// [`GhciWatch::restart_ghciwatch`] is called.
    ghciwatch: Session,
}

impl GhciWatch {
    async fn from_builder(mut builder: GhciWatchBuilder) -> eyre::Result<Self> {
        let ghc_version = FullGhcVersion::current()?;
        let tempdir = crate::internal::set_tempdir()?;
        let fs = Fs::new();
        write_cabal_config(&fs, &tempdir).await?;
        check_ghc_version(&tempdir, &ghc_version).await?;

        let inner_tempdir = tempdir.join("tmp");
        fs.create_dir(&inner_tempdir).await?;

        let paths_to_copy = vec![&builder.project_directory];
        tracing::info!(?paths_to_copy, "Copying project files");
        fs_extra::copy_items(&paths_to_copy, &tempdir, &Default::default())
            .wrap_err("Failed to copy project files")?;

        let project_directory_name = builder.project_directory.file_name().ok_or_else(|| {
            eyre!(
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

        tracing::info!("Starting ghciwatch");
        let mut repl_command = vec!["cabal".into(), format!("--with-compiler=ghc-{ghc_version}")];
        repl_command.extend(builder.cabal_args.iter().cloned());
        repl_command.push(format!(
            "--repl-options={}",
            shell_words::join(&builder.ghc_args)
        ));
        repl_command.push("v2-repl".into());
        repl_command.push(builder.cabal_target.clone());

        let repl_command = shell_words::join(repl_command);

        let command = ClonableCommand::new(test_bin::get_test_bin("ghciwatch").get_program())
            .arg("--log-json")
            .arg(&log_path)
            .args([
                "--command",
                &repl_command,
                "--watch",
                "src",
                "--watch",
                "my-simple-package.cabal", // This is going to get me in trouble.
                "--log-filter",
                &builder.get_log_filters(),
                "--log-filter-json",
                &builder.get_json_log_filters(),
                "--trace-spans",
                "new,close",
                "--poll",
                "1000ms",
            ])
            .args(builder.ghciwatch_args)
            .current_dir(&cwd)
            .env("HOME", &tempdir)
            .env("CABAL_DIR", tempdir.join(".cabal"))
            .env("TMPDIR", &inner_tempdir)
            // GHC will quote things with Unicode quotes unless we set this variable.
            // Very cute.
            // https://gitlab.haskell.org/ghc/ghc/-/blob/288235bbe5a59b8a1bda80aaacd59e5717417726/compiler/GHC/Driver/Session.hs#L1084-L1085
            // https://gitlab.haskell.org/ghc/ghc/-/blob/288235bbe5a59b8a1bda80aaacd59e5717417726/compiler/GHC/Utils/Outputable.hs#L728-L740
            .env("GHC_NO_UNICODE", "1")
            .stderr(clonable_command::Stdio::Inherit)
            .stdout(clonable_command::Stdio::Inherit);

        let session = Session::new(&command, builder.default_timeout, &log_path).await?;

        let events = Vec::with_capacity(1024);

        Ok(Self {
            command,
            log_path,
            cwd,
            events,
            ghc_version,
            default_timeout: builder.default_timeout,
            startup_timeout: builder.startup_timeout,
            fs: Fs::new(),
            ghciwatch: session,
        })
    }

    /// Start a new `ghciwatch` session in a copy of the given path.
    pub async fn new(project_directory: impl AsRef<Path>) -> eyre::Result<Self> {
        GhciWatchBuilder::new(project_directory).start().await
    }

    /// Clear all previously-read events.
    ///
    /// This makes previously-read events invisible to [`GhciWatch::assert_logged_or_wait`] and
    /// similar methods.
    pub fn clear_events(&mut self) {
        self.events.clear();
    }

    /// Read an event from the `ghciwatch` session.
    async fn read_event(&mut self) -> eyre::Result<&Event> {
        let event = self.ghciwatch.tracing_reader.next_event().await?;
        self.events.push(event);
        Ok(self.events.last().expect("We just inserted this event"))
    }

    /// Find a matching event in previously-read events.
    ///
    /// Returns the first matching event, or `None` if no matching events were found.
    fn find_logged(&self, matcher: &mut dyn Matcher) -> eyre::Result<Option<&Event>> {
        for event in &self.events {
            if matcher.matches(event)? {
                return Ok(Some(event));
            }
        }
        Ok(None)
    }

    /// Assert that a matching event was logged.
    pub fn assert_logged(&self, matcher: impl IntoMatcher) -> eyre::Result<&Event> {
        let mut matcher = matcher.into_matcher()?;
        self.find_logged(&mut matcher)?
            .ok_or_else(|| eyre!("No log message matching {matcher} found"))
    }

    /// Wait until a matching log event is found, with the given timeout.
    ///
    /// If `check_existing` is true, first check previously-read events before waiting for
    /// new ones.
    ///
    /// Errors if waiting for the event takes longer than the given `timeout`.
    async fn wait_for_log_with_timeout_inner(
        &mut self,
        matcher: impl IntoMatcher,
        check_existing: bool,
        timeout_duration: Duration,
    ) -> eyre::Result<Event> {
        let timeout_duration = timeout_mult(timeout_duration)?;

        let mut matcher = matcher.into_matcher()?;

        if check_existing {
            if let Some(event) = self.find_logged(&mut matcher)? {
                return Ok(event.clone());
            }
        }

        match tokio::time::timeout(timeout_duration, async {
            loop {
                let event = self.read_event().await?;
                if matcher.matches(event)? {
                    return Ok(event.clone());
                }
            }
        })
        .await
        {
            Ok(Ok(event)) => Ok(event),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(eyre!(
                "Waiting for a log message matching {matcher} \
                 timed out after {timeout_duration:.2?}"
            )),
        }
    }

    /// Wait until a matching log event is found, with the given timeout.
    ///
    /// Errors if waiting for the event takes longer than the given `timeout`.
    pub async fn wait_for_log_with_timeout(
        &mut self,
        matcher: impl IntoMatcher,
        timeout_duration: Duration,
    ) -> eyre::Result<Event> {
        self.wait_for_log_with_timeout_inner(matcher, false, timeout_duration)
            .await
    }

    /// Assert that a message matching `matcher` has been logged or wait for the
    /// `default_timeout` for a matching message to be logged.
    pub async fn assert_logged_or_wait(
        &mut self,
        matcher: impl IntoMatcher,
    ) -> eyre::Result<Event> {
        self.wait_for_log_with_timeout_inner(matcher, true, self.default_timeout)
            .await
    }

    /// Wait until a matching log event is found with the `default_timeout`.
    pub async fn wait_for_log(&mut self, matcher: impl IntoMatcher) -> eyre::Result<Event> {
        self.wait_for_log_with_timeout_inner(matcher, false, self.default_timeout)
            .await
    }

    /// Wait for a matching log event with the `startup_timeout`, checking previously-read
    /// events first.
    pub async fn wait_for_startup_log(&mut self, matcher: impl IntoMatcher) -> eyre::Result<Event> {
        self.wait_for_log_with_timeout_inner(matcher, true, self.startup_timeout)
            .await
    }

    /// Wait until `ghciwatch` completes its initial load.
    ///
    /// Returns immediately if `ghciwatch` has already completed its initial load.
    pub async fn wait_until_started(&mut self) -> eyre::Result<()> {
        self.wait_for_log_with_timeout_inner(
            BaseMatcher::ghci_started(),
            true,
            self.startup_timeout,
        )
        .await
        .wrap_err("ghciwatch didn't start in time")?;
        Ok(())
    }

    /// Wait until `ghciwatch` is ready to receive file events.
    ///
    /// Returns immediately if `ghciwatch` has already become ready to receive file events.
    pub async fn wait_until_watcher_started(&mut self) -> eyre::Result<()> {
        self.wait_for_log_with_timeout_inner(
            BaseMatcher::watcher_started(),
            true,
            self.default_timeout,
        )
        .await
        .wrap_err("notify watcher didn't start in time")?;
        Ok(())
    }

    /// Wait until `ghciwatch` completes its initial load and is ready to receive file events.
    ///
    /// Returns immediately if `ghciwatch` has already completed its initial load and become ready
    /// to receive file events.
    pub async fn wait_until_ready(&mut self) -> eyre::Result<()> {
        self.wait_for_log_with_timeout_inner(
            BaseMatcher::ghci_started().and(BaseMatcher::watcher_started()),
            true,
            self.startup_timeout,
        )
        .await
        .wrap_err("ghciwatch didn't start in time")?;
        Ok(())
    }

    /// Wait until `ghciwatch` reloads the `ghci` session due to changed modules.
    pub async fn wait_until_reload(&mut self) -> eyre::Result<()> {
        // TODO: It would be nice to verify which modules are changed.
        self.wait_for_log(BaseMatcher::ghci_reload()).await?;
        Ok(())
    }

    /// Wait until `ghciwatch` adds new modules to the `ghci` session.
    pub async fn wait_until_add(&mut self) -> eyre::Result<()> {
        // TODO: It would be nice to verify which modules are being added.
        self.wait_for_log(BaseMatcher::ghci_add()).await?;
        Ok(())
    }

    /// Wait until `ghciwatch` restarts the `ghci` session.
    pub async fn wait_until_restart(&mut self) -> eyre::Result<()> {
        // TODO: It would be nice to verify which modules have been deleted/moved.
        self.wait_for_log(BaseMatcher::ghci_restart()).await?;
        Ok(())
    }

    /// Wait until `ghciwatch` exits and return its status.
    pub async fn wait_until_exit(&self) -> eyre::Result<ExitStatus> {
        let mut child = crate::internal::take_ghciwatch_process()?;

        let status = child
            .wait()
            .await
            .wrap_err("Failed to wait for `ghciwatch` to exit")?;

        // Put it back.
        crate::internal::set_ghciwatch_process(child)?;

        Ok(status)
    }

    /// Restart the `ghciwatch` session.
    pub async fn restart_ghciwatch(&mut self) -> eyre::Result<()> {
        let child = crate::internal::take_ghciwatch_process()?;
        crate::internal::send_signal(&child, nix::sys::signal::Signal::SIGINT)?;
        // Put it back.
        crate::internal::set_ghciwatch_process(child)?;

        self.wait_until_exit().await?;

        // Get rid of it again or `Session::new` errors.
        let _ = crate::internal::take_ghciwatch_process()?;

        self.ghciwatch = Session::new(&self.command, self.default_timeout, &self.log_path).await?;

        self.clear_events();
        Ok(())
    }

    /// Get a path relative to the project root.
    pub fn path(&self, path: impl AsRef<Path>) -> PathBuf {
        self.cwd.join(path)
    }

    /// Get the major GHC version this test is running under.
    pub fn ghc_version(&self) -> GhcVersion {
        self.ghc_version.major
    }

    /// Get the PID of the `ghciwatch` process running for this test.
    pub fn pid(&self) -> u32 {
        self.ghciwatch.pid
    }

    /// Get the filesystem helpers.
    pub fn fs(&self) -> &Fs {
        &self.fs
    }

    /// Get a mutable reference to the filesystem helpers.
    pub fn fs_mut(&mut self) -> &mut Fs {
        &mut self.fs
    }
}

/// Write an empty `~/.cabal/config` so that `cabal` doesn't try to access the internet.
///
/// See: <https://github.com/haskell/cabal/issues/6167>
async fn write_cabal_config(fs: &Fs, home: &Path) -> eyre::Result<()> {
    std::fs::create_dir_all(home.join(".cabal")).wrap_err("Failed to create `.cabal` directory")?;
    fs.touch(home.join(".cabal/config"))
        .await
        .wrap_err("Failed to write empty `.cabal/config`")?;
    Ok(())
}

/// Check that `ghc-{ghc_version} --version` executes successfully.
///
/// This is a nice check that the given GHC version is present in the environment, to fail tests
/// early without waiting for `ghciwatch` to fail.
async fn check_ghc_version(home: &Path, ghc_version: &FullGhcVersion) -> eyre::Result<()> {
    let _output = Command::new(format!("ghc-{ghc_version}"))
        .env("HOME", home)
        .output()
        .await
        .wrap_err_with(|| format!("Failed to find GHC {ghc_version}"))?;
    // `ghc --version` returns a nonzero status code. As long as we could actually execute it, it's
    // OK if it failed.
    Ok(())
}
