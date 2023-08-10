//! The core [`Ghci`] session struct.

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::fmt::Display;
use std::process::Stdio;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Instant;

use aho_corasick::AhoCorasick;
use camino::Utf8PathBuf;
use itertools::Itertools;
use miette::IntoDiagnostic;
use miette::WrapErr;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::Child;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tokio::task;
use tokio::task::JoinHandle;
use tracing::instrument;

mod stdin;
use stdin::GhciStdin;
use stdin::StdinEvent;

mod stdout;
use stdout::GhciStdout;
use stdout::StdoutEvent;

mod stderr;
use stderr::GhciStderr;

mod show_modules;
use show_modules::ModuleSet;

use crate::aho_corasick::AhoCorasickExt;
use crate::buffers::LINE_BUFFER_CAPACITY;
use crate::event_filter::FileEvent;
use crate::incremental_reader::IncrementalReader;
use crate::sync_sentinel::SyncSentinel;

/// The `ghci` prompt we use. Should be unique enough, but maybe we can make it better with Unicode
/// private-use-area codepoints or something in the future.
pub const PROMPT: &str = "###~GHCID-NG-PROMPT~###";

/// The name we import `System.IO` as in `ghci`. This is used to run a few `putStrLn` commands and
/// similar without messing with the user's namespace. If you have a module in your project named
/// `GHCID_NG_IO_INTERNAL__` that's on you.
pub const IO_MODULE_NAME: &str = "GHCID_NG_IO_INTERNAL__";

/// A `ghci` session.
pub struct Ghci {
    /// A function which returns the command used to start this `ghci` session.
    /// This needs to be an [`Arc`] because [`Command`] doesn't implement [`Clone`] and we need to
    /// use this command to construct a new [`Ghci`] when we restart the `ghci` session.
    command: Arc<Mutex<Command>>,
    /// The running `ghci` process.
    process: Child,
    /// The handle for the stdout reader task.
    stdout: JoinHandle<miette::Result<()>>,
    /// The handle for the stderr reader task.
    stderr: JoinHandle<miette::Result<()>>,
    /// The handle for the stdin interaction task.
    stdin: JoinHandle<miette::Result<()>>,
    /// A channel for sending events to interact with the stdin task.
    stdin_channel: mpsc::Sender<StdinEvent>,
    /// Count of 'sync' events sent. This lets us sync stdin/stdout -- we write a message to stdin
    /// instructing `ghci` to print a sentinel string, and wait to read that string on `stdout`.
    sync_count: AtomicUsize,
    /// The currently-loaded modules in this `ghci` session.
    modules: ModuleSet,
    /// Path to write errors to, if any. Like `ghcid.txt`.
    error_path: Option<Utf8PathBuf>,
    /// `ghci` commands to run on startup.
    setup_commands: Vec<String>,
    /// `ghci` command to run tests.
    test_command: Option<String>,
}

impl Debug for Ghci {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Ghci").field(&self.process).finish()
    }
}

impl Ghci {
    /// Start a new `ghci` session using the given `command` to start `ghci`.
    ///
    /// This starts a number of asynchronous tasks to manage the `ghci` session's input and output
    /// streams.
    #[instrument(skip_all, level = "debug", name = "ghci")]
    pub async fn new(
        command_arc: Arc<Mutex<Command>>,
        error_path: Option<Utf8PathBuf>,
        setup_commands: Vec<String>,
        test_command: Option<String>,
    ) -> miette::Result<Arc<Mutex<Self>>> {
        let start_instant = Instant::now();

        let mut child = {
            let mut command = command_arc.lock().await;

            command
                .stdin(Stdio::piped())
                .stderr(Stdio::piped())
                .stdout(Stdio::piped())
                .kill_on_drop(true);

            command.spawn().into_diagnostic().wrap_err_with(|| {
                format!("Failed to start `{}`", crate::command::format(&command))
            })?
        };

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        // TODO: Is this a good capacity? Maybe it should just be 1.
        let (stdin_sender, stdin_receiver) = mpsc::channel(8);
        let (stdout_sender, stdout_receiver) = mpsc::channel(8);
        let (stderr_sender, stderr_receiver) = mpsc::channel(8);

        // So we want to put references to the `Ghci` struct we return in our tasks, but we don't
        // have that struct yet. So we create some trivial tasks to construct a valid `Ghci`, and
        // then create weak pointers to it and swap out the tasks.
        let stdout_handle = task::spawn(async { Ok(()) });
        let stderr_handle = task::spawn(async { Ok(()) });
        let stdin_handle = task::spawn(async { Ok(()) });

        let ret = Arc::new(Mutex::new(Ghci {
            command: command_arc,
            process: child,
            stdout: stdout_handle,
            stderr: stderr_handle,
            stdin: stdin_handle,
            stdin_channel: stdin_sender.clone(),
            sync_count: AtomicUsize::new(0),
            modules: Default::default(),
            error_path: error_path.clone(),
            setup_commands: setup_commands.clone(),
            test_command,
        }));

        // Three tasks for my three beautiful streams.
        let stdout = task::spawn(
            GhciStdout {
                reader: IncrementalReader::new(stdout).with_writer(tokio::io::stdout()),
                stderr_sender: stderr_sender.clone(),
                receiver: stdout_receiver,
                buffer: vec![0; LINE_BUFFER_CAPACITY],
                prompt_patterns: AhoCorasick::from_anchored_patterns([PROMPT]),
                mode: Mode::Compiling,
            }
            .run(),
        );
        let stderr = task::spawn(
            GhciStderr {
                reader: BufReader::new(stderr).lines(),
                receiver: stderr_receiver,
                compilation_summary: String::new(),
                buffers: BTreeMap::from([
                    (Mode::Compiling, String::with_capacity(LINE_BUFFER_CAPACITY)),
                    (Mode::Testing, String::with_capacity(LINE_BUFFER_CAPACITY)),
                ]),
                error_path,
                mode: Mode::Compiling,
                has_unwritten_data: false,
            }
            .run(),
        );
        let stdin = task::spawn(
            GhciStdin {
                stdin,
                stdout_sender: stdout_sender.clone(),
                stderr_sender: stderr_sender.clone(),
                receiver: stdin_receiver,
            }
            .run(),
        );

        // Now, replace the `JoinHandle`s with the actual values.
        {
            let mut ret = ret.lock().await;
            ret.stdout = stdout;
            ret.stderr = stderr;
            ret.stdin = stdin;
        };

        // Wait for the stdout job to start up.
        {
            let span = tracing::debug_span!("Stdout startup");
            let _enter = span.enter();
            let (sender, receiver) = oneshot::channel();
            stdout_sender
                .send(StdoutEvent::Initialize(sender))
                .await
                .into_diagnostic()?;
            receiver.await.into_diagnostic()?;
        }

        // Perform start-of-session initialization.
        {
            let span = tracing::debug_span!("Start-of-session initialization");
            let _enter = span.enter();
            let (sender, receiver) = oneshot::channel();
            stdin_sender
                .send(StdinEvent::Initialize {
                    sender,
                    setup_commands,
                })
                .await
                .into_diagnostic()?;
            receiver.await.into_diagnostic()?;
        }

        {
            let span = tracing::debug_span!("Start-of-session sync");
            let _enter = span.enter();
            // Sync up for any prompts.
            let mut guard = ret.lock().await;
            guard.sync().await?;
            // Get the initial list of loaded modules.
            guard.refresh_modules().await?;
        }

        tracing::info!("ghci started in {:.2?}", start_instant.elapsed());

        // Run the user-provided test command, if any.
        ret.lock().await.test().await?;

        Ok(ret)
    }

    /// Reload this `ghci` session to include the given modified and removed paths.
    ///
    /// This may fully restart the `ghci` process.
    #[instrument(skip_all, level = "debug")]
    pub async fn reload(
        this: Arc<Mutex<Self>>,
        events: Vec<FileEvent>,
    ) -> miette::Result<Arc<Mutex<Self>>> {
        // TODO: This method is pretty big -- we should break it up.

        // Once we know which paths were modified and which paths were removed, we can combine
        // that with information about this `ghci` session to determine which modules need to be
        // reloaded, which modules need to be added, and which modules were removed. In the case
        // of removed modules, the entire `ghci` session must be restarted.
        let mut needs_restart = Vec::new();
        let mut needs_reload = Vec::new();
        let mut add = Vec::new();
        {
            let guard = this.lock().await;
            for event in events {
                match event {
                    FileEvent::Remove(path) => {
                        // `ghci` can't cope with removed modules, so we need to fully restart the
                        // `ghci` process in case any modules are removed or renamed.
                        //
                        // https://gitlab.haskell.org/ghc/ghc/-/issues/11596
                        //
                        // TODO: I should investigate if `:unadd` works for some classes of removed
                        // modules.
                        tracing::debug!(?path, "Needs restart");
                        needs_restart.push(path);
                        break;
                    }
                    FileEvent::Modify(path) => {
                        if guard.modules.contains_source_path(&path) {
                            // We can `:reload` paths `ghci` already has loaded.
                            tracing::debug!(?path, "Needs reload");
                            needs_reload.push(path);
                        } else {
                            // Otherwise we need to `:add` the new paths.
                            tracing::debug!(?path, "Needs add");
                            add.push(path);
                        }
                    }
                }
            }
        }

        if !needs_restart.is_empty() {
            tracing::info!(
                "Restarting `ghci` due to deleted/moved modules:\n{}",
                format_bulleted_list(&needs_restart)
            );
            // TODO: Probably also need a restart hook / `.cabal` hook / similar.
            let mut guard = this.lock().await;
            guard.stop().await?;
            let command = guard.command.clone();
            return Self::new(
                command,
                guard.error_path.clone(),
                guard.setup_commands.clone(),
                guard.test_command.clone(),
            )
            .await;
        }

        let needs_add_or_reload = !add.is_empty() || !needs_reload.is_empty();
        let mut compilation_failed = needs_add_or_reload;

        if !add.is_empty() {
            tracing::info!(
                "Adding new modules to `ghci`:\n{}",
                format_bulleted_list(&add)
            );
            for path in add {
                let add_result = this.lock().await.add_module(path).await?;
                if let Some(Err(())) = add_result {
                    compilation_failed = false;
                }
            }
        }

        if !needs_reload.is_empty() {
            tracing::info!(
                "Reloading `ghci` due to changed modules:\n{}",
                format_bulleted_list(&needs_reload)
            );
            let (sender, receiver) = oneshot::channel();
            let guard = this.lock().await;
            guard
                .stdin_channel
                .send(StdinEvent::Reload(sender))
                .await
                .into_diagnostic()?;
            let reload_result = receiver.await.into_diagnostic()?;
            if let Some(Err(())) = reload_result {
                compilation_failed = false;
            }
        }

        if needs_add_or_reload {
            if compilation_failed {
                tracing::debug!("Compilation failed, skipping running tests.");
            } else {
                // If we loaded or reloaded any modules, we should run tests.
                let (sender, receiver) = oneshot::channel();
                let guard = this.lock().await;
                guard
                    .stdin_channel
                    .send(StdinEvent::Test {
                        sender,
                        test_command: guard.test_command.clone(),
                    })
                    .await
                    .into_diagnostic()?;
                receiver.await.into_diagnostic()?;
            }
        }

        this.lock().await.sync().await?;

        Ok(this)
    }

    /// Sync the input and output streams of this `ghci` session. This will block until all input
    /// written to the `ghci` process's stdin has been read and processed.
    #[instrument(skip_all, level = "debug")]
    pub async fn sync(&self) -> miette::Result<()> {
        let (sentinel, receiver) = SyncSentinel::new(&self.sync_count);
        self.stdin_channel
            .send(StdinEvent::Sync(sentinel))
            .await
            .into_diagnostic()?;
        receiver.await.into_diagnostic()?;
        Ok(())
    }

    /// Run the user provided test command.
    #[instrument(skip_all, level = "debug")]
    pub async fn test(&self) -> miette::Result<()> {
        let (sender, receiver) = oneshot::channel();
        self.stdin_channel
            .send(StdinEvent::Test {
                sender,
                test_command: self.test_command.clone(),
            })
            .await
            .into_diagnostic()?;
        receiver.await.into_diagnostic()?;
        Ok(())
    }

    /// Refresh the listing of loaded modules by parsing the `:show modules` output.
    #[instrument(skip_all, level = "debug")]
    pub async fn refresh_modules(&mut self) -> miette::Result<()> {
        let (sender, receiver) = oneshot::channel();
        self.stdin_channel
            .send(StdinEvent::ShowModules(sender))
            .await
            .into_diagnostic()?;
        let map = receiver.await.into_diagnostic()?;
        self.modules = map;
        tracing::debug!(
            "Parsed loaded modules, {} modules loaded",
            self.modules.len()
        );
        Ok(())
    }

    /// `:add` a module to the `ghci` session by path.
    ///
    /// Optionally returns a compilation result.
    #[instrument(skip(self), level = "debug")]
    pub async fn add_module(
        &mut self,
        path: Utf8PathBuf,
    ) -> miette::Result<Option<Result<(), ()>>> {
        let (sender, receiver) = oneshot::channel();
        self.stdin_channel
            .send(StdinEvent::AddModule(path.clone(), sender))
            .await
            .into_diagnostic()?;
        // TODO: What if adding the new module fails?
        self.modules.insert_source_path(path);
        receiver.await.into_diagnostic()
    }

    /// Stop this `ghci` session and cancel the async tasks associated with it.
    #[instrument(skip_all, level = "debug")]
    async fn stop(&mut self) -> miette::Result<()> {
        // TODO: Worth canceling the `mpsc::Receiver`s in the tasks here?
        // I'd need to add events for it.
        self.stdout.abort();
        self.stderr.abort();
        self.stdin.abort();

        // Kill the old `ghci` process.
        // TODO: Worth trying `SIGINT` or closing stdin here?
        self.process.kill().await.into_diagnostic()?;

        Ok(())
    }
}

/// The mode a `ghci` session is in. This is used to track output, particularly for the error log
/// (`ghcid.txt`).
///
/// Note: The [`Ord`] implementation on this type determines the order in which sections will be
/// written to the error log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mode {
    /// We're doing something private, like initializing the session, refreshing the list of loaded
    /// modules, etc.
    ///
    /// Stderr messages sent when the session is in this mode are not written to the error log.
    Internal,

    /// Compiling, loading, reloading, adding modules. We expect chunks of this output to end with
    /// a string like this before the prompt:
    ///
    /// 1. `Ok, [0-9]+ modules loaded.`
    /// 2. `Failed, [0-9]+ modules loaded.`
    Compiling,

    /// Running tests.
    Testing,
}

impl Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mode::Internal => write!(f, "internal"),
            Mode::Compiling => write!(f, "compilation"),
            Mode::Testing => write!(f, "test"),
        }
    }
}

fn format_bulleted_list(items: &[impl Display]) -> String {
    if items.is_empty() {
        String::new()
    } else {
        format!("• {}", items.iter().join("\n• "))
    }
}
