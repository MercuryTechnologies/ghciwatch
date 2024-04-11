use std::ffi::OsStr;
use std::ffi::OsString;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command as StdCommand;
use std::process::ExitStatus;
use std::time::Duration;

use clonable_command::Command as ClonableCommand;
use futures_util::future::BoxFuture;
use itertools::Itertools;
use miette::miette;
use miette::Context;
use miette::IntoDiagnostic;
use tap::Conv;
use tokio::process::Command;

use crate::matcher::Matcher;
use crate::tracing_reader::TracingReader;
use crate::BaseMatcher;
use crate::Checkpoint;
use crate::CheckpointIndex;
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
    make_args: Vec<String>,
    ghc_args: Vec<String>,
    cabal_args: Vec<String>,
    #[allow(clippy::type_complexity)]
    before_start: Option<Box<dyn FnOnce(PathBuf) -> BoxFuture<'static, miette::Result<()>> + Send>>,
    default_timeout: Duration,
    startup_timeout: Duration,
    log_filters: Vec<String>,
}

impl GhciWatchBuilder {
    /// Create a new builder for a `ghciwatch` session with the given project directory.
    pub fn new(project_directory: impl AsRef<Path>) -> Self {
        Self {
            project_directory: project_directory.as_ref().to_owned(),
            ghciwatch_args: Default::default(),
            ghc_args: Default::default(),
            make_args: Default::default(),
            cabal_args: Default::default(),
            before_start: None,
            default_timeout: Duration::from_secs(10),
            startup_timeout: Duration::from_secs(60),
            log_filters: Default::default(),
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

    /// Add an argument to the `make` invocations.
    pub fn with_make_arg(mut self, arg: impl AsRef<str>) -> Self {
        self.make_args.push(arg.as_ref().to_owned());
        self
    }

    /// Add multiple arguments to the `make` invocations.
    pub fn with_make_args(mut self, args: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.make_args
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

    /// Add a hook to run after project files are copied to the temporary directory but before
    /// `ghciwatch` is started.
    pub fn before_start<F>(mut self, before_start: impl Fn(PathBuf) -> F + Send + 'static) -> Self
    where
        F: Future<Output = miette::Result<()>> + Send + 'static,
    {
        self.before_start = Some(Box::new(move |path| Box::pin(before_start(path))));
        self
    }

    /// Set the default timeout to wait for log messages in [`GhciWatch::wait_for_log`],
    /// [`GhciWatch::assert_logged_or_wait`], and similar.
    ///
    /// The timeout defaults to 10 seconds.
    pub fn with_default_timeout(mut self, default_timeout: Duration) -> Self {
        self.default_timeout = default_timeout;
        self
    }

    /// Set the default timeout to wait for `ghci` to start up in
    /// [`GhciWatch::wait_until_started`] and [`GhciWatch::wait_until_ready`].
    ///
    /// The timeout defaults to 60 seconds.
    pub fn with_startup_timeout(mut self, startup_timeout: Duration) -> Self {
        self.startup_timeout = startup_timeout;
        self
    }

    /// Start `ghciwatch`.
    pub async fn start(self) -> miette::Result<GhciWatch> {
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
    ) -> miette::Result<Self> {
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
            .into_diagnostic()
            .wrap_err("Failed to start `ghciwatch`")?;

        let creates_log_path = fs.wait_for_path(timeout, log_path);
        tokio::select! {
            child_result = child.wait() => {
                return match child_result {
                    Err(err) => {
                        Err(err).into_diagnostic().wrap_err("ghciwatch failed to execute")
                    }
                    Ok(status) => {
                        Err(miette!("ghciwatch exited: {status}"))
                    }
                }
            }
            log_path_result = creates_log_path => {
                log_path_result?;
            }
            else => {}
        }

        let pid = child
            .id()
            .ok_or_else(|| miette!("`ghciwatch` has no PID"))?;

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
    events: Vec<Vec<Event>>,
    /// The version of GHC this test is running under.
    ghc_version: FullGhcVersion,
    /// The default timeout for waiting for log messages.
    default_timeout: Duration,
    /// The timeout for waiting for `ghci to finish starting up.
    startup_timeout: Duration,
    /// Filesystem helpers.
    fs: Fs,
    /// Data for this particular `ghciwatch` run. This changes when [`GhciWatch::restart`] is
    /// called.
    ghciwatch: Session,
}

impl GhciWatch {
    async fn from_builder(mut builder: GhciWatchBuilder) -> miette::Result<Self> {
        let ghc_version = FullGhcVersion::current()?;
        let tempdir = crate::internal::set_tempdir()?;
        let fs = Fs::new();
        write_cabal_config(&fs, &tempdir).await?;
        check_ghc_version(&tempdir, &ghc_version).await?;

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

        tracing::info!("Starting ghciwatch");
        let repl_command = shell_words::join(
            [
                "make",
                "ghci",
                &format!("GHC=ghc-{ghc_version}"),
                &format!("EXTRA_GHC_OPTS={}", shell_words::join(builder.ghc_args)),
                &format!("CABAL_OPTS={}", shell_words::join(builder.cabal_args)),
            ]
            .into_iter()
            .chain(builder.make_args.iter().map(|s| s.as_str())),
        );

        let log_filters = ["ghciwatch::watcher=trace", "ghciwatch=debug"]
            .into_iter()
            .chain(builder.log_filters.iter().map(|s| s.as_ref()))
            .join(",");

        let command = ClonableCommand::new(test_bin::get_test_bin("ghciwatch").get_program())
            .arg("--log-json")
            .arg(&log_path)
            .args([
                "--command",
                &repl_command,
                "--before-startup-shell",
                "hpack --force .",
                "--log-filter",
                &log_filters,
                "--trace-spans",
                "new,close",
                "--poll",
                "1000ms",
            ])
            .args(builder.ghciwatch_args)
            .current_dir(&cwd)
            .env("HOME", &tempdir)
            // GHC will quote things with Unicode quotes unless we set this variable.
            // Very cute.
            // https://gitlab.haskell.org/ghc/ghc/-/blob/288235bbe5a59b8a1bda80aaacd59e5717417726/compiler/GHC/Driver/Session.hs#L1084-L1085
            // https://gitlab.haskell.org/ghc/ghc/-/blob/288235bbe5a59b8a1bda80aaacd59e5717417726/compiler/GHC/Utils/Outputable.hs#L728-L740
            .env("GHC_NO_UNICODE", "1")
            .stderr(clonable_command::Stdio::Inherit)
            .stdout(clonable_command::Stdio::Inherit);

        let session = Session::new(&command, builder.default_timeout, &log_path).await?;

        // Most tests won't use checkpoints, so we'll only have a couple checkpoint slots
        // and many event slots in the first checkpoint chunk.
        let mut events = Vec::with_capacity(8);
        events.push(Vec::with_capacity(1024));

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
    pub async fn new(project_directory: impl AsRef<Path>) -> miette::Result<Self> {
        GhciWatchBuilder::new(project_directory).start().await
    }

    /// Get the first [`Checkpoint`].
    ///
    /// There is always an initial checkpoint which events are logged into before other
    /// checkpoints are created.
    ///
    /// Note that `first_checkpoint()..=current_checkpoint()` is equivalent to `..`.
    pub fn first_checkpoint(&self) -> Checkpoint {
        Checkpoint(0)
    }

    /// Get the current [`Checkpoint`].
    ///
    /// Events read by [`GhciWatch::wait_for_log_with_timeout`] and friends will add events to
    /// this checkpoint.
    pub fn current_checkpoint(&self) -> Checkpoint {
        Checkpoint(self.events.len() - 1)
    }

    /// Create and return a new [`Checkpoint`].
    ///
    /// New log events will be stored in this checkpoint.
    ///
    /// Later, you can check for log events in checkpoints with
    /// [`Self::assert_logged_in_checkpoint`] and friends.
    pub fn checkpoint(&mut self) -> Checkpoint {
        self.events.push(Vec::with_capacity(512));
        self.current_checkpoint()
    }

    /// Get the `Vec` of events since the last checkpoint.
    fn current_chunk_mut(&mut self) -> &mut Vec<Event> {
        self.events
            .last_mut()
            .expect("There is always an initial checkpoint")
    }

    /// Get an iterator over the events in the given checkpoints.
    ///
    /// The `index` can be an individual [`Checkpoint`] or any [`std::ops::Range`] of checkpoints.
    fn events_in_checkpoints(&self, index: impl CheckpointIndex) -> impl Iterator<Item = &Event> {
        self.events[index.as_index()].iter().flatten()
    }

    /// Read an event from the `ghciwatch` session.
    async fn read_event(&mut self) -> miette::Result<&Event> {
        let event = self.ghciwatch.tracing_reader.next_event().await?;
        let chunk = self.current_chunk_mut();
        chunk.push(event);
        Ok(chunk.last().expect("We just inserted this event"))
    }

    /// Find a matching event logged in one of the given `checkpoints`.
    ///
    /// Returns the first matching event, or `None` if no matching events were found.
    pub fn find_logged_in_checkpoint(
        &self,
        checkpoints: impl CheckpointIndex,
        matcher: impl IntoMatcher,
    ) -> miette::Result<Option<&Event>> {
        let mut ret = None;
        let mut matcher = matcher.into_matcher()?;
        for event in self.events_in_checkpoints(checkpoints) {
            if matcher.matches(event)? && ret.is_none() {
                ret = Some(event)
            }
        }

        Ok(ret)
    }

    /// Assert that a matching event was logged in one of the given `checkpoints`.
    ///
    /// Returns the first matching event.
    pub fn assert_logged_in_checkpoint(
        &self,
        checkpoints: impl CheckpointIndex + Clone,
        matcher: impl IntoMatcher,
    ) -> miette::Result<&Event> {
        let mut matcher = matcher.into_matcher()?;
        self.find_logged_in_checkpoint(&checkpoints, &mut matcher)?
            .ok_or_else(|| {
                miette!(
                    "No log message matching {matcher} found in checkpoint {:?}",
                    checkpoints.as_index()
                )
            })
    }

    /// Assert that a matching event was logged since the last [`Checkpoint`].
    pub fn assert_logged(&self, matcher: impl IntoMatcher) -> miette::Result<&Event> {
        self.assert_logged_in_checkpoint(self.current_checkpoint(), matcher)
    }

    /// Match a log message in the given checkpoints or wait until a matching log event is
    /// found.
    ///
    /// If `checkpoints` is `None`, do not check the `matcher` against any previously logged
    /// events.
    ///
    /// Errors if waiting for the event takes longer than the given `timeout`.
    pub async fn wait_for_log_with_timeout<M: IntoMatcher, C: CheckpointIndex>(
        &mut self,
        matcher: M,
        checkpoints: Option<C>,
        timeout_duration: Duration,
    ) -> miette::Result<Event> {
        let mut matcher = matcher.into_matcher()?;

        // First check if it was logged in `checkpoints`.
        if let Some(checkpoints) = checkpoints {
            if let Some(event) = self.find_logged_in_checkpoint(checkpoints, &mut matcher)? {
                return Ok(event.clone());
            }
        }

        // Otherwise, wait for a log message.
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
            Err(_) => Err(miette!(
                "Waiting for a log message matching {matcher} \
                 timed out after {timeout_duration:.2?}"
            )),
        }
    }

    /// Assert that a message matching `matcher` has been logged in the given [`Checkpoint`]s or
    /// wait for the `default_timeout` for a matching message to be logged.
    pub async fn assert_logged_in_checkpoint_or_wait(
        &mut self,
        checkpoints: impl CheckpointIndex,
        matcher: impl IntoMatcher,
    ) -> miette::Result<Event> {
        self.wait_for_log_with_timeout(matcher, Some(checkpoints), self.default_timeout)
            .await
    }

    /// Assert that a message matching `matcher` has been logged in the most recent [`Checkpoint`]
    /// or wait for the `default_timeout` for a matching message to be logged.
    pub async fn assert_logged_or_wait(
        &mut self,
        matcher: impl IntoMatcher,
    ) -> miette::Result<Event> {
        self.wait_for_log_with_timeout(
            matcher,
            Some(self.current_checkpoint()),
            self.default_timeout,
        )
        .await
    }

    /// Wait until a matching log event is found with the `default_timeout`.
    pub async fn wait_for_log(&mut self, matcher: impl IntoMatcher) -> miette::Result<Event> {
        self.wait_for_log_with_timeout(matcher, None::<Checkpoint>, self.default_timeout)
            .await
    }

    /// Wait until `ghciwatch` completes its initial load.
    ///
    /// Returns immediately if `ghciwatch` has already completed its initial load in the current
    /// checkpoint.
    pub async fn wait_until_started(&mut self) -> miette::Result<()> {
        self.wait_for_log_with_timeout(
            BaseMatcher::ghci_started(),
            Some(self.current_checkpoint()),
            self.startup_timeout,
        )
        .await
        .wrap_err("ghciwatch didn't start in time")?;
        Ok(())
    }

    /// Wait until `ghciwatch` is ready to receive file events.
    ///
    /// Returns immediately if `ghciwatch` has already become ready to receive file events in the
    /// current checkpoint.
    pub async fn wait_until_watcher_started(&mut self) -> miette::Result<()> {
        self.wait_for_log_with_timeout(
            BaseMatcher::watcher_started(),
            Some(self.current_checkpoint()),
            self.default_timeout,
        )
        .await
        .wrap_err("notify watcher didn't start in time")?;
        Ok(())
    }

    /// Wait until `ghciwatch` completes its initial load and is ready to receive file events.
    ///
    /// Returns immediately if `ghciwatch` has already completed its inital load and become ready to
    /// receive file events in the current checkpoint.
    pub async fn wait_until_ready(&mut self) -> miette::Result<()> {
        self.wait_for_log_with_timeout(
            BaseMatcher::ghci_started().and(BaseMatcher::watcher_started()),
            Some(self.current_checkpoint()),
            self.startup_timeout,
        )
        .await
        .wrap_err("ghciwatch didn't start in time")?;
        Ok(())
    }

    /// Wait until `ghciwatch` reloads the `ghci` session due to changed modules.
    pub async fn wait_until_reload(&mut self) -> miette::Result<()> {
        // TODO: It would be nice to verify which modules are changed.
        self.wait_for_log(BaseMatcher::ghci_reload()).await?;
        Ok(())
    }

    /// Wait until `ghciwatch` adds new modules to the `ghci` session.
    pub async fn wait_until_add(&mut self) -> miette::Result<()> {
        // TODO: It would be nice to verify which modules are being added.
        self.wait_for_log(BaseMatcher::ghci_add()).await?;
        Ok(())
    }

    /// Wait until `ghciwatch` restarts the `ghci` session.
    pub async fn wait_until_restart(&mut self) -> miette::Result<()> {
        // TODO: It would be nice to verify which modules have been deleted/moved.
        self.wait_for_log(BaseMatcher::ghci_restart()).await?;
        Ok(())
    }

    /// Wait until `ghciwatch` exits and return its status.
    pub async fn wait_until_exit(&self) -> miette::Result<ExitStatus> {
        let mut child = crate::internal::take_ghciwatch_process()?;

        let status = child
            .wait()
            .await
            .into_diagnostic()
            .wrap_err("Failed to wait for `ghciwatch` to exit")?;

        // Put it back.
        crate::internal::set_ghciwatch_process(child)?;

        Ok(status)
    }

    /// Restart the `ghciwatch` session.
    ///
    /// This creates and returns a new [`Checkpoint`].
    pub async fn restart_ghciwatch(&mut self) -> miette::Result<Checkpoint> {
        let child = crate::internal::take_ghciwatch_process()?;
        crate::internal::send_signal(&child, nix::sys::signal::Signal::SIGINT)?;
        // Put it back.
        crate::internal::set_ghciwatch_process(child)?;

        self.wait_until_exit().await?;

        // Get rid of it again or `Session::new` errors.
        let _ = crate::internal::take_ghciwatch_process()?;

        self.ghciwatch = Session::new(&self.command, self.default_timeout, &self.log_path).await?;

        Ok(self.checkpoint())
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
async fn write_cabal_config(fs: &Fs, home: &Path) -> miette::Result<()> {
    std::fs::create_dir_all(home.join(".cabal"))
        .into_diagnostic()
        .wrap_err("Failed to create `.cabal` directory")?;
    fs.touch(home.join(".cabal/config"))
        .await
        .wrap_err("Failed to write empty `.cabal/config`")?;
    Ok(())
}

/// Check that `ghc-{ghc_version} --version` executes successfully.
///
/// This is a nice check that the given GHC version is present in the environment, to fail tests
/// early without waiting for `ghciwatch` to fail.
async fn check_ghc_version(home: &Path, ghc_version: &FullGhcVersion) -> miette::Result<()> {
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
