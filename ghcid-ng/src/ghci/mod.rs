//! The core [`Ghci`] session struct.

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::fmt::Display;
use std::process::Stdio;
use std::sync::atomic::AtomicUsize;
use std::time::Instant;

use aho_corasick::AhoCorasick;
use camino::Utf8Path;
use camino::Utf8PathBuf;
use itertools::Itertools;
use miette::IntoDiagnostic;
use miette::WrapErr;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::Child;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task;
use tokio::task::JoinHandle;
use tracing::instrument;

mod stdin;
use stdin::GhciStdin;

mod stdout;
use stdout::GhciStdout;

mod stderr;
use stderr::GhciStderr;

pub mod parse;
use parse::CompilationResult;
use parse::GhcMessage;
use parse::ModuleSet;
use parse::Severity;

use crate::aho_corasick::AhoCorasickExt;
use crate::buffers::LINE_BUFFER_CAPACITY;
use crate::cli::Opts;
use crate::command;
use crate::command::ClonableCommand;
use crate::event_filter::FileEvent;
use crate::incremental_reader::IncrementalReader;
use crate::sync_sentinel::SyncSentinel;

use self::stderr::StderrEvent;

/// The `ghci` prompt we use. Should be unique enough, but maybe we can make it better with Unicode
/// private-use-area codepoints or something in the future.
pub const PROMPT: &str = "###~GHCID-NG-PROMPT~###";

/// The name we import `System.IO` as in `ghci`. This is used to run a few `putStrLn` commands and
/// similar without messing with the user's namespace. If you have a module in your project named
/// `GHCID_NG_IO_INTERNAL__` that's on you.
pub const IO_MODULE_NAME: &str = "GHCID_NG_IO_INTERNAL__";

/// Options for constructing a [`Ghci`]. This is like a lower-effort builder interface, mostly provided
/// because Rust tragically lacks named arguments.
///
/// Some of the other `*Opts` structs include borrowed data from the [`Opts`] struct, but this one
/// is fully owned; ultimately, this is because the [`watchexec::config::RuntimeConfig::on_action`]
/// takes an owned value. If we ever move to using something like the `notify` crate directly, we
/// could consider making this struct borrowed.
#[derive(Debug, Clone)]
pub struct GhciOpts {
    /// The command used to start the underlying `ghci` session.
    pub command: ClonableCommand,
    /// A path to write `ghci` errors to.
    pub error_path: Option<Utf8PathBuf>,
    /// Shell commands to run before starting or restarting `ghci`.
    pub before_startup_shell: Vec<ClonableCommand>,
    /// `ghci` commands to run after starting or restarting `ghci`.
    pub after_startup_ghci: Vec<String>,
    /// `ghci` command which runs tests.
    pub test_ghci: Option<String>,
}

impl GhciOpts {
    /// Construct options for [`Ghci`] from parsed command-line interface arguments as [`Opts`].
    ///
    /// This extracts the bits of an [`Opts`] struct relevant to the [`Ghci`] session without
    /// cloning or taking ownership of the entire thing.
    pub fn from_cli(opts: &Opts) -> miette::Result<Self> {
        // TODO: implement fancier default command
        // See: https://github.com/ndmitchell/ghcid/blob/e2852979aa644c8fed92d46ab529d2c6c1c62b59/src/Ghcid.hs#L142-L171
        let command = opts
            .command
            .clone()
            .unwrap_or_else(|| ClonableCommand::new("cabal").arg("repl"));

        Ok(Self {
            command,
            error_path: opts.errors.clone(),
            before_startup_shell: opts.before_startup_shell.clone(),
            after_startup_ghci: opts.after_startup_ghci.clone(),
            test_ghci: opts.test_ghci.clone(),
        })
    }
}

/// A `ghci` session.
pub struct Ghci {
    /// Options used to start this `ghci` session. We keep this around so we can reuse it when
    /// restarting this session.
    opts: GhciOpts,
    /// The running `ghci` process.
    process: Child,
    /// The handle for the stderr reader task.
    stderr_handle: JoinHandle<miette::Result<()>>,
    /// The stdin writer.
    stdin: GhciStdin,
    /// The stdout reader.
    stdout: GhciStdout,
    /// Channel for communicating with the stderr reader task.
    stderr: mpsc::Sender<StderrEvent>,
    /// Count of 'sync' events sent. This lets us sync stdin/stdout -- we write a message to stdin
    /// instructing `ghci` to print a sentinel string, and wait to read that string on `stdout`.
    sync_count: AtomicUsize,
    /// The currently-loaded modules in this `ghci` session.
    modules: ModuleSet,
    /// Modules that have failed to compile in this `ghci` session.
    ///
    /// These don't show up in `:show modules` and aren't, technically speaking, loaded, but we
    /// also get an error if we `:add` them due to [GHC bug #13254][ghc-13254], so we track them
    /// here.
    ///
    /// [ghc-13254]: https://gitlab.haskell.org/ghc/ghc/-/issues/13254
    failed_modules: ModuleSet,
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
    pub async fn new(opts: GhciOpts) -> miette::Result<Self> {
        let start_instant = Instant::now();

        {
            let span = tracing::debug_span!("before_startup_shell");
            let _enter = span.enter();
            for command in &opts.before_startup_shell {
                let program = &command.program;
                let mut command = command.as_tokio();
                let command_formatted = command::format(&command);
                tracing::info!("$ {command_formatted}");
                let status = command.status().await.into_diagnostic().wrap_err_with(|| {
                    format!("Failed to execute `{}`", command::format(&command))
                })?;
                if status.success() {
                    tracing::debug!("{program:?} exited successfully: {status}");
                } else {
                    tracing::error!("{program:?} failed: {status}");
                }
            }
        }

        let mut child = {
            let mut command = opts.command.as_tokio();

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
        let (stderr_sender, stderr_receiver) = mpsc::channel(8);

        // So we want to put references to the `Ghci` struct we return in our tasks, but we don't
        // have that struct yet. So we create some trivial tasks to construct a valid `Ghci`, and
        // then create weak pointers to it and swap out the tasks.
        let stderr_handle = task::spawn(async { Ok(()) });

        let stdout = GhciStdout {
            reader: IncrementalReader::new(stdout).with_writer(tokio::io::stdout()),
            stderr_sender: stderr_sender.clone(),
            buffer: vec![0; LINE_BUFFER_CAPACITY],
            prompt_patterns: AhoCorasick::from_anchored_patterns([PROMPT]),
            mode: Mode::Compiling,
        };

        let stdin = GhciStdin {
            stdin,
            stderr_sender: stderr_sender.clone(),
        };

        let mut ret = Ghci {
            opts,
            process: child,
            stderr_handle,
            stdin,
            stdout,
            stderr: stderr_sender,
            sync_count: AtomicUsize::new(0),
            modules: Default::default(),
            failed_modules: Default::default(),
        };

        let stderr = task::spawn(
            GhciStderr {
                reader: BufReader::new(stderr).lines(),
                receiver: stderr_receiver,
                compilation_summary: String::new(),
                buffers: BTreeMap::from([
                    (Mode::Compiling, String::with_capacity(LINE_BUFFER_CAPACITY)),
                    (Mode::Testing, String::with_capacity(LINE_BUFFER_CAPACITY)),
                ]),
                buffer: String::with_capacity(LINE_BUFFER_CAPACITY),
                error_path: ret.opts.error_path.clone(),
                mode: Mode::Compiling,
                has_unwritten_data: false,
            }
            .run(),
        );

        // Now, replace the `JoinHandle`s with the actual values.
        {
            ret.stderr_handle = stderr;
        };

        // Wait for the stdout job to start up.
        let messages = ret.stdout.initialize().await?;
        ret.process_ghc_messages(messages).await?;

        // Perform start-of-session initialization.
        ret.stdin
            .initialize(&mut ret.stdout, &ret.opts.after_startup_ghci)
            .await?;

        // Sync up for any prompts.
        ret.sync().await?;
        // Get the initial list of loaded modules.
        ret.refresh_modules().await?;

        tracing::info!("ghci started in {:.2?}", start_instant.elapsed());

        // Run the user-provided test command, if any.
        ret.test().await?;

        Ok(ret)
    }

    async fn get_reload_actions(&self, events: Vec<FileEvent>) -> miette::Result<ReloadActions> {
        // Once we know which paths were modified and which paths were removed, we can combine
        // that with information about this `ghci` session to determine which modules need to be
        // reloaded, which modules need to be added, and which modules were removed. In the case
        // of removed modules, the entire `ghci` session must be restarted.
        let mut needs_restart = Vec::new();
        let mut needs_reload = Vec::new();
        let mut needs_add = Vec::new();
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
                    tracing::debug!(%path, "Needs restart");
                    needs_restart.push(path);
                }
                FileEvent::Modify(path) => {
                    if self.modules.contains_source_path(&path)?
                        || self.failed_modules.contains_source_path(&path)?
                    {
                        // We can `:reload` paths `ghci` already has loaded.
                        tracing::debug!(%path, "Needs reload");
                        needs_reload.push(path);
                    } else {
                        // Otherwise we need to `:add` the new paths.
                        tracing::debug!(%path, "Needs add");
                        needs_add.push(path);
                    }
                }
            }
        }

        Ok(ReloadActions {
            needs_restart,
            needs_reload,
            needs_add,
        })
    }

    /// Reload this `ghci` session to include the given modified and removed paths.
    ///
    /// This may fully restart the `ghci` process.
    #[instrument(skip_all, level = "debug")]
    pub async fn reload(&mut self, events: Vec<FileEvent>) -> miette::Result<()> {
        let actions = self.get_reload_actions(events).await?;

        if !actions.needs_restart.is_empty() {
            tracing::info!(
                "Restarting ghci due to deleted/moved modules:\n{}",
                format_bulleted_list(&actions.needs_restart)
            );
            self.stop().await?;
            let new = Self::new(self.opts.clone()).await?;
            let _ = std::mem::replace(self, new);
        }

        let mut compilation_failed = false;

        if !actions.needs_add.is_empty() {
            tracing::info!(
                "Adding new modules to ghci:\n{}",
                format_bulleted_list(&actions.needs_add)
            );
            for path in &actions.needs_add {
                let add_result = self.add_module(path).await?;
                if let Some(CompilationResult::Err) = add_result {
                    compilation_failed = true;
                }
            }
        }

        if !actions.needs_reload.is_empty() {
            tracing::info!(
                "Reloading ghci due to changed modules:\n{}",
                format_bulleted_list(&actions.needs_reload)
            );
            let messages = self.stdin.reload(&mut self.stdout).await?;
            if let Some(CompilationResult::Err) = self.process_ghc_messages(messages).await? {
                compilation_failed = true;
            }
        }

        if actions.needs_add_or_reload() {
            if compilation_failed {
                tracing::debug!("Compilation failed, skipping running tests.");
            } else {
                // If we loaded or reloaded any modules, we should run tests.
                self.test().await?;
            }
        }

        self.sync().await?;

        Ok(())
    }

    /// Sync the input and output streams of this `ghci` session. This will block until all input
    /// written to the `ghci` process's stdin has been read and processed.
    #[instrument(skip_all, level = "debug")]
    pub async fn sync(&mut self) -> miette::Result<()> {
        let (sentinel, receiver) = SyncSentinel::new(&self.sync_count);
        self.stdin.sync(&mut self.stdout, sentinel).await?;
        receiver.await.into_diagnostic()?;
        Ok(())
    }

    /// Run the user provided test command.
    #[instrument(skip_all, level = "debug")]
    pub async fn test(&mut self) -> miette::Result<()> {
        self.stdin
            .test(&mut self.stdout, self.opts.test_ghci.clone())
            .await?;
        Ok(())
    }

    /// Refresh the listing of loaded modules by parsing the `:show modules` output.
    #[instrument(skip_all, level = "debug")]
    pub async fn refresh_modules(&mut self) -> miette::Result<()> {
        let map = self.stdin.show_modules(&mut self.stdout).await?;
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
        path: &Utf8Path,
    ) -> miette::Result<Option<CompilationResult>> {
        let messages = self.stdin.add_module(&mut self.stdout, path).await?;

        let result = self.process_ghc_messages(messages).await?;

        if let Some(CompilationResult::Ok) = result {
            self.modules.insert_source_path(path)?;
        }
        // Otherwise, compilation failed or otherwise didn't print a summary, so we don't want to
        // add the module to the module set.

        Ok(result)
    }

    /// Stop this `ghci` session and cancel the async tasks associated with it.
    #[instrument(skip_all, level = "debug")]
    async fn stop(&mut self) -> miette::Result<()> {
        // TODO: Worth canceling the `mpsc::Receiver`s in the tasks here?
        // I'd need to add events for it.
        self.stderr_handle.abort();

        // Kill the old `ghci` process.
        // TODO: Worth trying `SIGINT` or closing stdin here?
        self.process.kill().await.into_diagnostic()?;

        Ok(())
    }

    /// Processes a set of diagnostics and messages parsed from GHC output.
    #[instrument(skip_all, level = "trace")]
    async fn process_ghc_messages(
        &mut self,
        messages: Vec<GhcMessage>,
    ) -> miette::Result<Option<CompilationResult>> {
        for message in messages {
            match message {
                GhcMessage::Compiling(module) => {
                    tracing::debug!(module = %module.name, path = %module.path, "Compiling");
                    self.failed_modules.remove_source_path(&module.path)?;
                }
                GhcMessage::Diagnostic {
                    severity: Severity::Error,
                    path: Some(path),
                    message,
                    ..
                } => {
                    // We can't use 'message' for the field name because that's what tracing uses
                    // for the message.
                    tracing::debug!(%path, error = message, "Module failed to compile");
                    self.failed_modules.insert_source_path(&path)?;
                }
                GhcMessage::Summary { result, message } => {
                    match result {
                        CompilationResult::Ok => {
                            tracing::debug!("Compilation succeeded");
                        }
                        CompilationResult::Err => {
                            tracing::debug!("Compilation failed");
                        }
                    }

                    // Notify the stderr task of the compilation summary.
                    let (sender, receiver) = oneshot::channel();
                    self.stderr
                        .send(StderrEvent::SetCompilationSummary {
                            summary: message.clone(),
                            sender,
                        })
                        .await
                        .into_diagnostic()?;
                    receiver.await.into_diagnostic()?;
                }
                _ => {}
            }
        }

        Ok(None)
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

/// Actions needed to perform a reload.
///
/// See [`Ghci::reload`].
struct ReloadActions {
    /// Paths to modules which need a full `ghci` restart.
    needs_restart: Vec<Utf8PathBuf>,
    /// Paths to modules which need a `:reload`.
    needs_reload: Vec<Utf8PathBuf>,
    /// Paths to modules which need an `:add`.
    needs_add: Vec<Utf8PathBuf>,
}

impl ReloadActions {
    /// Do any modules need to be added or reloaded?
    fn needs_add_or_reload(&self) -> bool {
        !self.needs_add.is_empty() || !self.needs_reload.is_empty()
    }
}
