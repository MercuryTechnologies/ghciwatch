//! The core [`Ghci`] session struct.

use command_group::AsyncCommandGroup;
use nix::sys::signal;
use nix::sys::signal::Signal;
use owo_colors::OwoColorize;
use owo_colors::Stream::Stdout;
use std::borrow::Borrow;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::fmt::Debug;
use std::io::IsTerminal;
use std::process::ExitStatus;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;
use tokio::io::DuplexStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use aho_corasick::AhoCorasick;
use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use camino::Utf8Path;
use camino::Utf8PathBuf;
use eyre::eyre;
use eyre::WrapErr;
use nix::unistd::Pid;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::sync::mpsc;
use tracing::instrument;

mod stdin;
use stdin::GhciStdin;

mod stdout;
use stdout::GhciStdout;

mod stderr;
use stderr::GhciStderr;

mod process;
use process::GhciProcess;

pub mod manager;

mod error_log;
use error_log::ErrorLog;

pub mod parse;
use parse::parse_eval_commands;
use parse::CompilationResult;
use parse::EvalCommand;
use parse::ModulesLoaded;
use parse::ShowPaths;

mod ghci_command;
pub use ghci_command::GhciCommand;

mod compilation_log;
pub use compilation_log::CompilationLog;

mod writer;
use crate::buffers::GHCI_BUFFER_CAPACITY;
pub use crate::ghci::writer::GhciWriter;
use crate::haskell_source_file::is_haskell_source_file;

mod progress_writer;

mod module_set;
pub use module_set::ModuleSet;

mod file_classifier;
pub use file_classifier::FileClassifier;
use file_classifier::ReloadActions;

mod loaded_module;
use loaded_module::LoadedModule;

mod warning_formatter;

mod warning_tracker;
use warning_tracker::WarningTracker;

use crate::aho_corasick::AhoCorasickExt;
use crate::buffers::LINE_BUFFER_CAPACITY;
use crate::cli::ExperimentalFeature;
use crate::cli::Opts;
use crate::clonable_command::ClonableCommand;
use crate::event_filter::FileEvent;
use crate::format_bulleted_list;
use crate::hooks;
use crate::hooks::HookOpts;
use crate::hooks::LifecycleEvent;
use crate::ignore::GlobMatcher;
use crate::incremental_reader::IncrementalReader;
use crate::normal_path::NormalPath;
use crate::shutdown::ShutdownHandle;
use crate::CommandExt;
use crate::StringCase;

/// The `ghci` prompt we use. Should be unique enough, but maybe we can make it better with Unicode
/// private-use-area codepoints or something in the future.
pub const PROMPT: &str = "###~GHCIWATCH-PROMPT~###";

/// Options for constructing a [`Ghci`]. This is like a lower-effort builder interface, mostly provided
/// because Rust tragically lacks named arguments.
///
/// Some of the other `*Opts` structs include borrowed data from the [`Opts`] struct, but this one
/// is fully owned; ultimately, this is because [`Ghci`] is run through a [`ShutdownHandle`], which
/// requires that the task is fully owned.
#[derive(Debug, Clone)]
pub struct GhciOpts {
    /// The command used to start the underlying `ghci` session.
    pub command: ClonableCommand,
    /// A path to write `ghci` errors to.
    pub error_path: Option<Utf8PathBuf>,
    /// Enable running eval commands in files.
    pub enable_eval: bool,
    /// Extra directories to add to the module import search paths parsed from `:show paths`,
    /// used for converting module paths to module names and vice versa.
    pub extra_search_paths: Vec<Utf8PathBuf>,
    /// Lifecycle hooks, mostly `ghci` commands to run at certain points.
    pub hooks: HookOpts,
    /// Restart the `ghci` session when paths matching these globs are changed.
    pub restart_globs: GlobMatcher,
    /// Reload the `ghci` session when paths matching these globs are changed.
    pub reload_globs: GlobMatcher,
    /// Determines whether we should interrupt a reload in progress or not.
    pub interrupt_reloads: bool,
    /// Where to write what `ghci` emits to `stdout`. Inherits parent's `stdout` by default.
    pub stdout_writer: GhciWriter,
    /// Where to write what `ghci` emits to `stderr`. Inherits parent's `stderr` by default.
    pub stderr_writer: GhciWriter,
    /// Whether to clear the screen before reloads and restarts.
    pub clear: bool,
    /// Whether to track warnings across recompilations.
    pub track_warnings: bool,
}

impl GhciOpts {
    /// Construct options for [`Ghci`] from parsed command-line interface arguments as [`Opts`].
    ///
    /// This extracts the bits of an [`Opts`] struct relevant to the [`Ghci`] session without
    /// cloning or taking ownership of the entire thing.
    ///
    /// If running in TUI mode, `ghci` output (from `stdout_writer` and `stderr_writer`) is sent to
    /// the stream given by the second return value.
    pub fn from_cli(opts: &Opts) -> eyre::Result<(Self, Option<DuplexStream>)> {
        // TODO: implement fancier default command
        // See: https://github.com/ndmitchell/ghcid/blob/e2852979aa644c8fed92d46ab529d2c6c1c62b59/src/Ghcid.hs#L142-L171
        let command = match (&opts.file, &opts.command) {
            (Some(file), None) => ClonableCommand::new("ghci").arg(file.relative()),
            (None, Some(command)) => command.clone(),
            (None, None) => ClonableCommand::new("cabal").arg("repl"),
            (Some(_), Some(_)) => unreachable!(),
        };

        enum OutputMode {
            Tui,
            Progress,
            Standard,
        }

        let mode = if opts.has_experimental_feature(ExperimentalFeature::Tui) {
            if opts.has_experimental_feature(ExperimentalFeature::Progress) {
                tracing::warn!(
                    "`--experimental-features tui` and `--experimental-features progress` \
                     are mutually exclusive; `progress` will be ignored in TUI mode"
                );
            }
            OutputMode::Tui
        } else if opts.has_experimental_feature(ExperimentalFeature::Progress)
            && std::io::stdout().is_terminal()
        {
            OutputMode::Progress
        } else {
            OutputMode::Standard
        };

        let stdout_writer;
        let stderr_writer;
        let tui_reader;

        match mode {
            OutputMode::Tui => {
                let (tui_writer, tui_reader_inner) = tokio::io::duplex(GHCI_BUFFER_CAPACITY);
                let tui_writer = GhciWriter::duplex_stream(tui_writer);
                stdout_writer = tui_writer.clone();
                stderr_writer = tui_writer.clone();
                tui_reader = Some(tui_reader_inner);
            }
            OutputMode::Progress => {
                stdout_writer = GhciWriter::stdout().with_progress(true);
                stderr_writer = GhciWriter::stderr();
                tui_reader = None;
            }
            OutputMode::Standard => {
                stdout_writer = GhciWriter::stdout();
                stderr_writer = GhciWriter::stderr();
                tui_reader = None;
            }
        }

        Ok((
            Self {
                command,
                error_path: opts.error_file.clone(),
                enable_eval: opts.enable_eval,
                extra_search_paths: opts
                    .extra_module_search_paths
                    .iter()
                    .map(|path| path.absolute().to_owned())
                    .collect(),
                hooks: opts.hooks.clone(),
                restart_globs: opts.watch.restart_globs()?,
                reload_globs: opts.watch.reload_globs()?,
                interrupt_reloads: opts.interrupt_reloads(),
                stdout_writer,
                stderr_writer,
                clear: opts.clear,
                track_warnings: opts.track_warnings,
            },
            tui_reader,
        ))
    }

    /// Create a [`FileClassifier`] from these options.
    ///
    /// The classifier uses the process's current working directory. Call
    /// [`FileClassifier::set_cwd`] after GHCi initialization to update it.
    pub fn file_classifier(&self) -> eyre::Result<FileClassifier> {
        FileClassifier::new(self.restart_globs.clone(), self.reload_globs.clone())
    }

    #[instrument(skip_all, level = "trace")]
    fn clear(&self) {
        if self.clear {
            tracing::trace!("Clearing the screen");
            if let Err(err) = clearscreen::clear() {
                tracing::debug!("Failed to clear the terminal: {err}");
            }
        }
    }
}

/// A `ghci` session.
pub struct Ghci {
    /// Options used to start this `ghci` session. We keep this around so we can reuse it when
    /// restarting this session.
    opts: GhciOpts,
    /// The shutdown handle, used for performing or responding to graceful shutdowns.
    shutdown: ShutdownHandle,
    /// The process group ID of the `ghci` session process.
    ///
    /// This is used to send the process `Ctrl-C` (`SIGINT`) to cancel reloads or other actions.
    process_group_id: Pid,
    /// The stdin writer.
    stdin: GhciStdin,
    /// The stdout reader.
    stdout: GhciStdout,
    /// Sender for notifying the process watching job ([`GhciProcess`]) that we're shutting down
    /// the `ghci` session on purpose. If the process watcher sees `ghci` exit, usually it will
    /// trigger a shutdown of the entire program. This is bad if we're restarting `ghci` on
    /// purpose, so this channel helps us avoid that.
    restart_sender: mpsc::Sender<()>,
    /// Sender for notifying [`run_ghci`][manager::run_ghci] when `ghci` exits unexpectedly.
    /// Cloned into each new [`GhciProcess`] on construction; kept alive here so the channel is
    /// never closed while this session is live.
    exited_sender: mpsc::Sender<ExitStatus>,
    /// Writer for `ghcid`-compatible output, useful for editor integration for diagnostics.
    error_log: ErrorLog,
    /// Classifies file events into reload actions based on glob patterns.
    classifier: FileClassifier,
    /// The set of targets for this `ghci` session, from `:show targets`.
    ///
    /// Targets that fail to compile don't show up in `:show modules` and aren't, technically
    /// speaking, loaded, but we also get an error if we `:add` them due to [GHC bug
    /// #13254][ghc-13254], so we track them here.
    ///
    /// [ghc-13254]: https://gitlab.haskell.org/ghc/ghc/-/issues/13254
    targets: ModuleSet,
    /// Eval commands, if `opts.enable_eval` is set.
    eval_commands: BTreeMap<NormalPath, Vec<EvalCommand>>,
    /// Search paths / current working directory for this `ghci` session.
    search_paths: ShowPaths,
    /// Tasks running `async:` shell commands in the background.
    command_handles: Vec<JoinHandle<eyre::Result<ExitStatus>>>,
    /// Monotonic counter for generating unique sync barrier nonces.
    sync_nonce: u64,
    /// Warning tracker for managing warnings across recompilations.
    warning_tracker: WarningTracker,
}

impl Debug for Ghci {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ghci")
            .field("pid", &self.process_group_id)
            .finish()
    }
}

impl Ghci {
    /// Start a new `ghci` session.
    ///
    /// This starts a number of asynchronous tasks to manage the `ghci` session's input and output
    /// streams.
    #[instrument(skip_all, level = "debug", name = "ghci")]
    pub async fn new(
        mut shutdown: ShutdownHandle,
        opts: GhciOpts,
        exited_sender: mpsc::Sender<ExitStatus>,
    ) -> eyre::Result<Self> {
        let mut command_handles = Vec::new();
        {
            let span = tracing::debug_span!("before_startup_shell");
            let _enter = span.enter();
            opts.hooks
                .run_shell_hooks(
                    LifecycleEvent::Startup(hooks::When::Before),
                    &mut command_handles,
                )
                .await?;
        }

        let mut group = {
            let mut command = opts.command.as_tokio();

            command
                .stdin(Stdio::piped())
                .stderr(Stdio::piped())
                .stdout(Stdio::piped())
                .kill_on_drop(true);

            command
                .group_spawn()
                .wrap_err_with(|| format!("Failed to start {}", command.display()))?
        };

        let process_group_id = Pid::from_raw(
            group
                .id()
                .ok_or_else(|| eyre!("ghci process has no process group ID"))? as i32,
        );

        let child = group.inner();
        let process_id = Pid::from_raw(
            child
                .id()
                .ok_or_else(|| eyre!("ghci process has no process ID"))? as i32,
        );
        tracing::debug!(
            pid = process_id.as_raw(),
            pgid = process_group_id.as_raw(),
            "Started ghci"
        );

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        // TODO: Is this a good capacity? Maybe it should just be 1.
        let (stderr_sender, stderr_receiver) = mpsc::channel(8);

        let stdout = GhciStdout {
            reader: IncrementalReader::new(stdout).with_writer(opts.stdout_writer.clone()),
            stderr_sender: stderr_sender.clone(),
            buffer: vec![0; LINE_BUFFER_CAPACITY],
            prompt_patterns: AhoCorasick::from_anchored_patterns([PROMPT]),
        };

        let stdin = GhciStdin { stdin };

        shutdown
            .spawn("stderr", |shutdown| {
                GhciStderr {
                    shutdown,
                    reader: BufReader::new(stderr).lines(),
                    writer: opts.stderr_writer.clone(),
                    receiver: stderr_receiver,
                    buffer: String::with_capacity(LINE_BUFFER_CAPACITY),
                }
                .run()
            })
            .await;

        let (restart_sender, restart_receiver) = mpsc::channel(1);

        shutdown
            .spawn("ghci_process", |shutdown| {
                GhciProcess {
                    shutdown,
                    restart_receiver,
                    process_group_id,
                    exited_sender: exited_sender.clone(),
                }
                .run(group)
            })
            .await;

        let error_log = ErrorLog::new(match &opts.error_path {
            Some(error_path) => Some(NormalPath::from_cwd(error_path)?),
            None => None,
        });
        let classifier =
            FileClassifier::new(opts.restart_globs.clone(), opts.reload_globs.clone())?;
        let extra_search_paths = opts.extra_search_paths.clone();

        Ok(Ghci {
            opts,
            shutdown: shutdown.clone(),
            process_group_id,
            stdin,
            stdout,
            restart_sender,
            exited_sender,
            error_log,
            classifier,
            targets: Default::default(),
            eval_commands: Default::default(),
            search_paths: ShowPaths {
                cwd: crate::current_dir_utf8()?,
                search_paths: extra_search_paths,
            },
            command_handles,
            sync_nonce: 0,
            warning_tracker: WarningTracker::new(),
        })
    }

    /// Perform post-startup initialization.
    ///
    /// Diagnostics will be added to the given `log`, and the error log will be written.
    #[instrument(level = "debug", skip_all)]
    pub async fn initialize<const N: usize>(
        &mut self,
        log: &mut CompilationLog,
        events: [LifecycleEvent; N],
    ) -> eyre::Result<()> {
        let start_instant = Instant::now();

        self.error_log.write_still_compiling().await?;

        // Don't propagate the error here immediately so we can be sure we always write the
        // compilation log.
        let result = self.initialize_inner(log).await;
        if let Err(err) = result.as_ref() {
            // If writing the compilation log or running hooks fails, we should log this error so
            // it's not lost forever.
            tracing::debug!("Initializing failed: {err}");
        }

        // If we're in `--repl-no-load`, we may not have gotten a summary message. In that case,
        // fill in an empty "All good (0 modules)" message.
        //
        // Note: We ONLY want to do this on startup.
        log.fill_empty_summary();
        self.finish_compilation(start_instant, log, events).await?;

        result
    }

    async fn initialize_inner(&mut self, log: &mut CompilationLog) -> eyre::Result<()> {
        // Wait for the stdout job to start up.
        self.stdout.initialize(log).await?;

        // Perform start-of-session initialization.
        self.stdin.initialize(&mut self.stdout, log).await?;

        // Get the initial list of targets.
        self.refresh_targets().await?;
        // Get the initial list of eval commands.
        self.refresh_eval_commands().await?;

        // For initialization, consider all targets as "changed"
        self.warning_tracker.reset_changed_files();
        for target in self.targets.iter() {
            self.warning_tracker
                .mark_file_changed(target.path().clone());
        }

        Ok(())
    }

    fn get_reload_actions(&self, events: BTreeSet<FileEvent>) -> eyre::Result<ReloadActions> {
        self.classifier.classify(events, &self.targets)
    }

    /// Reload this `ghci` session to include the given modified and removed paths.
    ///
    /// This may fully restart the `ghci` process.
    ///
    /// NOTE: We interrupt reloads when applicable, so this function may be canceled and dropped at
    /// any `await` point!
    #[instrument(skip_all, level = "debug")]
    pub async fn reload(
        &mut self,
        events: BTreeSet<FileEvent>,
        kind_sender: oneshot::Sender<GhciReloadKind>,
    ) -> eyre::Result<()> {
        let start_instant = Instant::now();
        let actions = self.get_reload_actions(events.clone())?;
        let _ = kind_sender.send(actions.kind());

        // Track which files were directly changed in this reload
        self.warning_tracker.reset_changed_files();
        for event in &events {
            let path = self.classifier.relative_path(event.as_path())?;
            self.warning_tracker.mark_file_changed(path);
        }

        if actions.needs_restart() {
            self.opts.clear();
            self.error_log.write_still_compiling().await?;
            tracing::info!(
                "Restarting ghci:\n{}",
                format_bulleted_list(&actions.needs_restart)
            );
            self.restart().await?;
            // Once we restart, everything is freshly loaded. We don't need to add or
            // reload any other modules.
            return Ok(());
        }

        let mut log = CompilationLog::default();

        if actions.needs_modify() {
            self.opts.clear();
            self.error_log.write_still_compiling().await?;
            self.run_hooks(LifecycleEvent::Reload(hooks::When::Before), &mut log)
                .await?;
        }

        if !actions.needs_remove.is_empty() {
            tracing::info!(
                "Removing modules from ghci:\n{}",
                format_bulleted_list(&actions.needs_remove)
            );
            self.remove_modules(&actions.needs_remove, &mut log).await?;
        }

        if !actions.needs_add.is_empty() {
            tracing::info!(
                "Adding modules to ghci:\n{}",
                format_bulleted_list(&actions.needs_add)
            );
            self.add_modules(&actions.needs_add, &mut log).await?;
        }

        if !actions.needs_reload.is_empty() {
            tracing::info!(
                "Reloading ghci:\n{}",
                format_bulleted_list(&actions.needs_reload)
            );
            self.stdin.reload(&mut self.stdout, &mut log).await?;
            self.refresh_eval_commands_for_paths(&actions.needs_reload)
                .await?;
        }

        if actions.needs_modify() {
            self.finish_compilation(
                start_instant,
                &mut log,
                [LifecycleEvent::Reload(hooks::When::After)],
            )
            .await?;
        }

        self.prune_command_handles();

        Ok(())
    }

    /// Restart the `ghci` session without triggering restart hooks.
    ///
    /// This is meant to be used when starting the `ghci` session itself fails; in this case, we
    /// don't have a prompt to write (e.g.) before-restart GHCi command hooks into, and we aren't
    /// really "restarting" a session so much as starting it again. That is, this method avoids
    /// "broken pipe" errors with `--before-restart-ghci` hooks.
    #[instrument(skip_all, level = "debug")]
    async fn startup_restart(&mut self) -> eyre::Result<()> {
        let mut log = CompilationLog::default();

        self.restart_inner(&mut log, [LifecycleEvent::Startup(hooks::When::After)])
            .await?;

        Ok(())
    }

    /// Restart the `ghci` session.
    #[instrument(skip_all, level = "debug")]
    async fn restart(&mut self) -> eyre::Result<()> {
        let mut log = CompilationLog::default();

        self.run_hooks(LifecycleEvent::Restart(hooks::When::Before), &mut log)
            .await?;
        self.restart_inner(
            &mut log,
            [
                LifecycleEvent::Startup(hooks::When::After),
                LifecycleEvent::Restart(hooks::When::After),
            ],
        )
        .await?;

        Ok(())
    }

    #[instrument(skip_all, level = "debug")]
    async fn restart_inner<const N: usize>(
        &mut self,
        log: &mut CompilationLog,
        events: [LifecycleEvent; N],
    ) -> eyre::Result<()> {
        self.stop().await?;
        let new = Self::new(
            self.shutdown.clone(),
            self.opts.clone(),
            self.exited_sender.clone(),
        )
        .await?;
        let _ = std::mem::replace(self, new);
        self.initialize(log, events).await?;

        Ok(())
    }

    /// Run the user provided test command.
    #[instrument(skip_all, level = "debug")]
    async fn test(&mut self, log: &mut CompilationLog) -> eyre::Result<()> {
        self.run_hooks(LifecycleEvent::Test, log).await?;
        Ok(())
    }

    /// Run the eval commands, if enabled.
    #[instrument(skip_all, level = "debug")]
    async fn eval(&mut self, log: &mut CompilationLog) -> eyre::Result<()> {
        if !self.opts.enable_eval {
            return Ok(());
        }

        // TODO: This `clone` is ugly but I can't get the borrow checker to accept it otherwise.
        // Might be more efficient to swap it out for a default, but then it gets trickier to
        // restore the old value when the function returns.
        for (path, commands) in self.eval_commands.clone() {
            // If we don't have any eval commands for this path, do nothing.
            if commands.is_empty() {
                continue;
            }

            // If the `module` was already compiled, `ghci` may have loaded the interface file instead
            // of the interpreted bytecode, giving us this error message when we attempt to
            // load the top-level scope with `:module + *{module}`:
            //
            //     module 'Mercury.Typescript.Golden' is not interpreted
            //
            // We use `:add *{module}` to force interpreting the module. We do this here instead of in
            // `add_module` to save time if eval commands aren't used (or aren't needed for a
            // particular module).
            tracing::info!("Loading {path} in interpreted mode for eval commands");
            self.interpret_module(&path, log).await?;
            let module = self.search_paths.path_to_module(&path)?;
            self.stdin
                .add_module_to_scope(&mut self.stdout, &module, log)
                .await?;
            for command in commands {
                tracing::info!("Eval {path}:{command}");
                self.stdin
                    .run_command(&mut self.stdout, &command.command, log)
                    .await?;
            }
            self.stdin
                .remove_module_from_scope(&mut self.stdout, &module, log)
                .await?;
        }

        Ok(())
    }

    /// Refresh the listing of targets by parsing the `:show paths` and `:show targets` output.
    #[instrument(skip_all, level = "debug")]
    async fn refresh_targets(&mut self) -> eyre::Result<()> {
        self.refresh_paths().await?;
        self.targets = self
            .stdin
            .show_targets(&mut self.stdout, &self.search_paths)
            .await?;
        tracing::debug!(targets = self.targets.len(), "Parsed targets");
        Ok(())
    }

    /// Refresh the listing of search paths by parsing the `:show paths` output.
    #[instrument(skip_all, level = "debug")]
    async fn refresh_paths(&mut self) -> eyre::Result<()> {
        self.search_paths = self.stdin.show_paths(&mut self.stdout).await?;
        for path in &self.opts.extra_search_paths {
            if !self.search_paths.search_paths.contains(path) {
                self.search_paths.search_paths.push(path.clone());
            }
        }
        self.classifier.set_cwd(self.search_paths.cwd.clone());
        tracing::debug!(cwd = %self.search_paths.cwd, search_paths = ?self.search_paths.search_paths, "Parsed paths");
        Ok(())
    }

    /// Refresh `eval_commands` by reading and parsing the files in `targets`.
    #[instrument(skip_all, level = "debug")]
    async fn refresh_eval_commands(&mut self) -> eyre::Result<()> {
        if !self.opts.enable_eval {
            return Ok(());
        }

        let mut eval_commands = BTreeMap::new();

        for target in self.targets.iter() {
            // Note: Loaded targets are always Haskell modules.
            let commands = Self::parse_eval_commands(target.path()).await?;
            if !commands.is_empty() {
                eval_commands.insert(target.path().clone(), commands);
            }
        }

        self.eval_commands = eval_commands;
        Ok(())
    }

    /// Refresh `eval_commands` by reading and parsing the given files.
    #[instrument(skip_all, level = "debug")]
    async fn refresh_eval_commands_for_paths(
        &mut self,
        paths: impl IntoIterator<Item = impl Borrow<NormalPath>>,
    ) -> eyre::Result<()> {
        if !self.opts.enable_eval {
            return Ok(());
        }

        for path in paths {
            let path = path.borrow();

            // To actually _execute_ eval commands with the proper bindings in scope, we need to be
            // able to evaluate (interpret) a file, which requires we know its module _name_
            // (because `:module + *MODULE_NAME` only supports module names and not source paths).
            //
            // We get _all_ file events in this loop, not just Haskell source files, so let's guard
            // adding an entry to the `eval_commands` map by making sure we can convert the path to
            // a module name.
            //
            // However!!! We're _modifying_ an existing map here, so if we look at a path and
            // _don't_ find any commands, we need to be careful to _remove_ that entry from the map.
            //
            // Hey maybe this should just be a generic multimap structure, anyone ever think of that?
            if self.search_paths.path_to_module(path).is_err() {
                if is_haskell_source_file(path) {
                    // If the path is a Haskell source file (ends with `.hs` or similar), we should
                    // warn the user directly. Otherwise, it's probably a `.persistentmodels` or
                    // something and the user (probably!) won't expect eval commands to be evaluated
                    // in it.
                    tracing::warn!(%path, "Could not determine module path, skipping parsing eval commands");
                } else {
                    tracing::debug!(%path, "Could not determine module path, skipping parsing eval commands");
                }
                self.eval_commands.remove(path);
                continue;
            }

            let commands = Self::parse_eval_commands(path).await?;
            if commands.is_empty() {
                self.eval_commands.remove(path);
            } else {
                self.eval_commands.insert(path.clone(), commands);
            }
        }

        Ok(())
    }

    /// Remove all `eval_commands` for the given paths.
    #[instrument(skip_all, level = "debug")]
    async fn clear_eval_commands_for_paths(
        &mut self,
        paths: impl IntoIterator<Item = impl Borrow<NormalPath>>,
    ) {
        if !self.opts.enable_eval {
            return;
        }

        for path in paths {
            self.eval_commands.remove(path.borrow());
        }
    }

    /// Read and parse eval commands from the given `path`.
    #[instrument(level = "trace")]
    async fn parse_eval_commands(path: &Utf8Path) -> eyre::Result<Vec<EvalCommand>> {
        let contents = tokio::fs::read_to_string(path)
            .await
            .wrap_err_with(|| format!("Failed to read {path}"))?;
        let commands = parse_eval_commands(&contents)
            .wrap_err_with(|| format!("Failed to parse eval commands from file {path}"))?;
        Ok(commands)
    }

    /// `:add` a module or modules to the GHCi session.
    #[instrument(skip(self), level = "debug")]
    async fn add_modules(
        &mut self,
        paths: &[NormalPath],
        log: &mut CompilationLog,
    ) -> eyre::Result<()> {
        let mut modules = Vec::with_capacity(paths.len());
        for path in paths {
            if self.targets.contains_source_path(path) {
                return Err(eyre!(
                    "Attempting to add already-loaded module: {path}\n\
                    This is a ghciwatch bug; please report it upstream"
                ));
            } else {
                modules.push(LoadedModule::new(path.clone()));
            }
        }

        self.stdin
            .add_modules(&mut self.stdout, &modules, log)
            .await?;

        // TODO: This could lead to the module set getting out of sync with the underlying GHCi
        // session.
        //
        // If there's a TOATOU bug here (e.g. we're attempting to add a module but the file no
        // longer exists), then we can get into a situation like this:
        //
        //     ghci> :add src/DoesntExist.hs src/MyLib.hs
        //     File src/DoesntExist.hs not found
        //     [4 of 4] Compiling MyLib        ( src/MyLib.hs, interpreted )
        //     Ok, four modules loaded.
        //
        //     ghci> :show targets
        //     src/MyLib.hs
        //     ...
        //
        // We've requested to load two modules, only one has been loaded, but GHCi has reported
        // that compilation was successful and hasn't added the failing module to the target set.
        // Note that if the file is found but compilation fails, the file _is_ added to the target
        // set:
        //
        //     ghci> :add src/MyCoolLib.hs
        //     [4 of 4] Compiling MyCoolLib        ( src/MyCoolLib.hs, interpreted )
        //
        //     src/MyCoolLib.hs:4:12: error:
        //         • Couldn't match expected type ‘IO ()’ with actual type ‘()’
        //         • In the expression: ()
        //           In an equation for ‘someFunc’: someFunc = ()
        //       |
        //     4 | someFunc = ()
        //       |            ^^
        //     Failed, three modules loaded.
        //
        //     ghci> :show targets
        //     src/MyCoolLib.hs
        //     ...
        //
        // I think this is OK, because the only reason we need to know which modules are loaded is
        // to avoid the "module defined in multiple files" bug [1], so the potential outcomes of
        // making this mistake are:
        //
        // 1. The next time the file is modified, we attempt to `:add` it instead of `:reload`ing
        //    it. This is harmless, though it changes the order that `:show modules` prints output
        //    in (maybe local binding order as well or something).
        // 2. The next time the file is modified, we attempt to `:add` it by path instead of by
        //    module name, but this function is only used when the modules aren't already in the
        //    target set, so we know the module doesn't need to be referred to by its module name.
        //
        // [1]: https://gitlab.haskell.org/ghc/ghc/-/issues/13254#note_525037

        self.targets.extend(modules);

        self.refresh_eval_commands_for_paths(paths).await?;

        Ok(())
    }

    /// `:add *` a module to the `ghci` session by path.
    ///
    /// This forces it to be interpreted.
    #[instrument(skip(self), level = "debug")]
    async fn interpret_module(
        &mut self,
        path: &NormalPath,
        log: &mut CompilationLog,
    ) -> eyre::Result<()> {
        let module = self.targets.get_import_name(path);

        self.stdin
            .interpret_module(&mut self.stdout, &module, log)
            .await?;

        // Note: A borrowed path is only returned if the path is already present in the module set.
        if let Cow::Owned(module) = module {
            self.targets.insert_module(module);
        }

        self.refresh_eval_commands_for_paths(std::iter::once(path))
            .await?;

        Ok(())
    }

    /// `:unadd` a module or modules from the `ghci` session by path.
    #[instrument(skip(self), level = "debug")]
    async fn remove_modules(
        &mut self,
        paths: &[NormalPath],
        log: &mut CompilationLog,
    ) -> eyre::Result<()> {
        let modules = paths
            .iter()
            .map(|path| self.targets.get_import_name(path).into_owned())
            .collect::<Vec<_>>();

        // Each `:unadd` implicitly reloads as well, so we have to `:unadd` all the modules in a
        // single command so that GHCi doesn't try to load a bunch of removed modules after each
        // one.
        self.stdin
            .remove_modules(&mut self.stdout, modules.iter().map(Borrow::borrow), log)
            .await?;

        for path in paths {
            self.targets.remove_source_path(path);
        }

        self.clear_eval_commands_for_paths(paths).await;
        if self.opts.track_warnings {
            self.warning_tracker.clear_warnings_for_paths(paths);
        }

        Ok(())
    }

    /// Stop this `ghci` session and cancel the async tasks associated with it.
    #[instrument(skip_all, level = "debug")]
    async fn stop(&mut self) -> eyre::Result<()> {
        // Tell the `GhciProcess` to shut down `ghci` without requesting a shutdown for
        // `ghciwatch`.
        let _ = self.restart_sender.try_send(());

        Ok(())
    }

    /// Interrupt the running GHCi session.
    ///
    /// On `Err`, the GHCi session may have been killed (e.g. because the sync
    /// barrier could not restore the prompt). Callers MUST treat an error here
    /// as a session-died event and route through the normal restart path
    /// rather than propagating it as fatal. See [`Ghci::sync_barrier`] for details.
    #[instrument(skip_all, level = "debug")]
    async fn send_sigint(&mut self) -> eyre::Result<()> {
        let start_instant = Instant::now();

        // Phase 1: Send SIGINT repeatedly until we find a clean, uninterrupted prompt.
        //
        // An interrupted reload can cause interleaved output between the GHCi prompt and
        // compilation output (due to GHC bug where the logging thread isn't stopped on
        // async exception — see `runParPipelines` in GHC's Driver/Make.hs). We send
        // SIGINT with exponential backoff until we see a prompt that isn't garbled.
        let mut backoff = ExponentialBackoff {
            initial_interval: Duration::from_millis(5),
            max_interval: Duration::from_millis(100),
            multiplier: 1.25,
            max_elapsed_time: Some(Duration::from_secs(10)),
            ..Default::default()
        };

        let mut sigint_count: usize = 0;
        loop {
            let Some(delay) = backoff.next_backoff() else {
                return Err(eyre!(
                    "Timed out waiting for GHCi to respond to SIGINT after {:.2?}",
                    start_instant.elapsed()
                ));
            };

            sigint_count += 1;
            signal::killpg(self.process_group_id, Signal::SIGINT)
                .wrap_err("Failed to send `Ctrl-C` (`SIGINT`) to ghci session")?;
            tracing::debug!(count = sigint_count, "Sent SIGINT");

            let found = self.stdout.buffer_and_drain_prompts(delay).await?;
            if found > 0 {
                tracing::debug!(
                    found,
                    elapsed = ?start_instant.elapsed(),
                    "Found prompt after SIGINT"
                );
                break;
            }
        }

        // If we only sent 1 SIGINT, then there cannot be extra prompts waiting to be read from the
        // buffer; only do the sync barrier process if we sent multiple SIGINTs.
        if sigint_count > 1 {
            self.sync_barrier().await?;
        }

        tracing::info!("Interrupted ghci in {:.2?}", start_instant.elapsed());
        Ok(())
    }

    /// Sync barrier: deterministically consume all stale prompts from the pipe.
    ///
    /// We rely on the fact that GHCi processes input commands one at a time, in order. When we send
    /// a command to GHCi, we read its output up until the next prompt and know that the output
    /// we've read matches the command we sent. This is important because we parse GHCi output in
    /// several places (e.g. compilation errors go to the `error_log`, `:show paths` and `:show
    /// targets` are used to inform module additions/removals/reloads, etc.), so if we're parsing
    /// output from a different command, we'll Have Problems.
    ///
    /// When we're hitting Ctrl-C repeatedly (in case of a user input prompt interleaved with
    /// compilation output in GHCi's stdout stream), we don't know how many times GHCi will print a
    /// prompt that we can read.
    ///
    /// Therefore, we _change_ the prompt and read until _that_ specific prompt shows up in the
    /// output, using a unique (to the `ghci` process) and different prompt each time we call this
    /// method. This ensures we consume all remaining stale output, without having to wait until we
    /// "think it's safe" and wasting the user's time after GHCi is done writing.
    #[instrument(skip_all, level = "debug")]
    async fn sync_barrier(&mut self) -> eyre::Result<()> {
        self.sync_nonce += 1;
        let nonce = self.sync_nonce;
        let sync_marker = format!("~~~GHCIWATCH-SYNC-{nonce}~~~");

        // Set the prompt to our sync marker.
        self.stdin
            .write_set_prompt(&sync_marker)
            .await
            .wrap_err("Failed to write sync command to ghci stdin")?;

        // From here until the prompt is restored, any failure leaves the session
        // unable to match `PROMPT` again. Restoring in-band after a failed read
        // is not safe (the buffer is in an unknown state, and confirming the
        // restore would itself depend on prompt matching), so on any error we
        // SIGKILL the process and let the manager restart the session.
        let sync_timeout = Duration::from_secs(3);
        let read =
            tokio::time::timeout(sync_timeout, self.stdout.read_until_marker(&sync_marker)).await;
        let result = match read {
            Ok(Ok(_ghci_output)) => self
                .stdin
                .set_prompt(
                    &mut self.stdout,
                    PROMPT,
                    crate::incremental_reader::FindAt::LineStart,
                    // We don't expect to see any compilation here, so we pass a stub
                    // `CompilationLog` and discard it.
                    &mut Default::default(),
                )
                .await
                .wrap_err("Failed to restore prompt after sync barrier"),
            Ok(Err(e)) => Err(e).wrap_err("Failed to read until sync marker"),
            Err(_elapsed) => Err(eyre!(
                "Timed out waiting for GHCi sync marker after {sync_timeout:?}"
            )),
        };

        if let Err(e) = result {
            // Kill the process directly rather than going through `restart_sender`.
            // `restart_sender` is the graceful-shutdown path: `GhciProcess` consumes it
            // and intentionally suppresses `exited_sender`, so the manager would never
            // learn ghci died. We need the wait future in `GhciProcess::run` to win the
            // select so `exited_sender` fires and `wait_and_restart_runtime` takes over.
            if let Err(kill_err) = signal::killpg(self.process_group_id, Signal::SIGKILL)
                .wrap_err("Failed to send `SIGKILL` to ghci session")
            {
                tracing::error!(
                    error = %kill_err,
                    "Failed to SIGKILL ghci after sync_barrier failure",
                );
            }
            return Err(e).wrap_err(
                "ghci sync barrier failed; killed the session because the prompt could not be restored",
            );
        }
        Ok(())
    }

    #[allow(dead_code)] // TODO: No it should not be!
    #[instrument(skip_all, level = "trace")]
    async fn before_startup_shell(command: &ClonableCommand) -> eyre::Result<()> {
        let program = &command.program;
        let mut command = command.as_tokio();
        command.kill_on_drop(true);
        let command_formatted = command.display();
        tracing::info!("$ {command_formatted}");
        let status = command
            .status()
            .await
            .wrap_err_with(|| format!("Failed to execute `{command_formatted}`"))?;
        if status.success() {
            tracing::debug!("{program:?} exited successfully: {status}");
        } else {
            tracing::error!("{program:?} failed: {status}");
        }
        Ok(())
    }

    // Get rid of any handles for background commands that have finished.
    #[instrument(skip_all, level = "trace")]
    fn prune_command_handles(&mut self) {
        self.command_handles.retain(|handle| !handle.is_finished());
    }

    /// Finish a compilation process.
    ///
    /// This outputs how long the compilation took (since `compilation_start`), runs eval and test
    /// commands (if compilation succeeded), and writes the error log.
    #[instrument(skip_all, level = "trace")]
    async fn finish_compilation<const N: usize>(
        &mut self,
        compilation_start: Instant,
        log: &mut CompilationLog,
        events: [LifecycleEvent; N],
    ) -> eyre::Result<()> {
        if let Some(error_log_dir) = self.error_log.path().and_then(|path| path.parent()) {
            log.relocate(&self.search_paths.cwd, error_log_dir)?;
        }

        // Update warnings from the compilation log only if tracking is enabled
        if self.opts.track_warnings {
            self.warning_tracker.update_warnings_from_log(log);
        }

        // Allow hooks to consume the error log by updating it before running the hooks.
        self.write_error_log(log).await?;

        for event in events {
            self.run_hooks(event, log).await?;
        }

        let event = events[N - 1];

        if let Some(CompilationResult::Err) = log.result() {
            tracing::error!(
                "{} failed in {:.2?}",
                event.event_noun().first_char_to_ascii_uppercase(),
                compilation_start.elapsed()
            );
        } else {
            // Display any tracked warnings even if compilation succeeded
            // but exclude files that were compiled in this cycle (to avoid duplicates)
            if self.opts.track_warnings {
                self.display_tracked_warnings_excluding_compiled(log).await;
            }

            let warning_count = if self.opts.track_warnings {
                self.warning_tracker.warning_count()
            } else {
                0
            };

            if warning_count > 0 {
                tracing::info!(
                    "{} Finished {} in {:.2?} ({} warning{} tracked)",
                    "Compilation succeeded".if_supports_color(Stdout, |text| text.yellow()),
                    event.event_noun(),
                    compilation_start.elapsed(),
                    warning_count,
                    if warning_count == 1 { "" } else { "s" }
                );
            } else {
                tracing::info!(
                    "{} Finished {} in {:.2?}",
                    "All good!".if_supports_color(Stdout, |text| text.green()),
                    event.event_noun(),
                    compilation_start.elapsed()
                );
            }

            // Run the eval commands, if any.
            self.eval(log).await?;
            // Run the user-provided test command, if any.
            self.test(log).await?;
        }

        Ok(())
    }

    #[instrument(skip_all, fields(%event), level = "trace")]
    async fn run_hooks(
        &mut self,
        event: LifecycleEvent,
        log: &mut CompilationLog,
    ) -> eyre::Result<()> {
        for hook in self.opts.hooks.select(event) {
            tracing::info!(command = %hook.command, "Running {hook} command");
            match &hook.command {
                hooks::Command::Ghci(command) => {
                    let start_time = Instant::now();
                    self.stdin
                        .run_command(&mut self.stdout, command, log)
                        .await?;
                    if let LifecycleEvent::Test = &hook.event {
                        tracing::info!("Finished running tests in {:.2?}", start_time.elapsed());
                    }
                }
                hooks::Command::Shell(command) => {
                    command.run_on(&mut self.command_handles).await?;
                }
            }
        }

        Ok(())
    }

    /// Display tracked warnings excluding files that were compiled in the current cycle.
    #[instrument(skip_all, level = "trace")]
    async fn display_tracked_warnings_excluding_compiled(&self, log: &CompilationLog) {
        if !self.opts.track_warnings {
            return;
        }

        // Create a set of file paths that were compiled in this cycle
        // Use relative paths for comparison since GHC reports relative paths
        let compiled_files: HashSet<_> = log
            .compiled_modules
            .iter()
            .map(|module| module.path.as_path())
            .collect();

        for (file_path, file_warnings) in self.warning_tracker.get_all_warnings() {
            // Skip warnings for files that were compiled in this cycle
            // Compare using relative paths since compilation logs use relative paths
            if compiled_files.contains(file_path.relative()) {
                continue;
            }

            for warning in file_warnings {
                warning.display_colored();
            }
        }
    }

    /// Display all tracked warnings to the user with GHC-matching colors.
    #[instrument(skip_all, level = "trace")]
    async fn display_tracked_warnings(&self) {
        // Single iteration - no need to check has_warnings() first
        for file_warnings in self.warning_tracker.get_all_warnings().values() {
            for warning in file_warnings {
                warning.display_colored();
            }
        }
    }

    #[instrument(skip(self), level = "trace")]
    async fn write_error_log(&mut self, log: &CompilationLog) -> eyre::Result<()> {
        if self.opts.track_warnings {
            self.write_error_log_with_tracked_warnings(log).await
        } else {
            self.error_log.write(log).await
        }
    }

    /// Write error log including tracked warnings from previous compilations.
    ///
    /// This method combines current compilation diagnostics with tracked warnings,
    /// avoiding duplicates and only including warnings (not errors) from tracked diagnostics.
    #[instrument(skip(self), level = "trace")]
    async fn write_error_log_with_tracked_warnings(
        &mut self,
        log: &CompilationLog,
    ) -> eyre::Result<()> {
        use crate::ghci::parse::Severity;
        use std::collections::HashSet;

        let path = match self.error_log.path() {
            Some(path) => path,
            None => {
                tracing::debug!("No error log path, not writing");
                return Ok(());
            }
        };

        let file = tokio::fs::File::create(path).await?;
        let mut writer = tokio::io::BufWriter::new(file);

        // Write compilation summary header if compilation succeeded
        if let Some(summary) = log.summary {
            if let CompilationResult::Ok = summary.result {
                tracing::debug!(%path, "Writing 'All good'");
                let modules_loaded = if summary.modules_loaded != ModulesLoaded::Count(1) {
                    format!("{} modules", summary.modules_loaded)
                } else {
                    format!("{} module", summary.modules_loaded)
                };
                writer
                    .write_all(format!("All good ({modules_loaded})\n").as_bytes())
                    .await?;
            }
        }

        // Write current compilation diagnostics
        for diagnostic in &log.diagnostics {
            tracing::debug!(%diagnostic, "Writing current compilation diagnostic");
            writer
                .write_all(diagnostic.to_string().as_bytes())
                .await?;
        }

        // Create a set of diagnostics from current compilation to avoid duplicates
        // We'll use a simple string-based deduplication approach
        let mut current_diagnostics: HashSet<String> = HashSet::new();
        for diagnostic in &log.diagnostics {
            current_diagnostics.insert(diagnostic.to_string());
        }

        // Write tracked warnings (only warnings, not errors) that are not already in current compilation
        for file_warnings in self.warning_tracker.get_all_warnings().values() {
            for warning in file_warnings {
                // Only include warnings, not errors
                if warning.severity != Severity::Warning {
                    continue;
                }

                let warning_str = warning.to_string();

                // Skip if this warning is already in the current compilation log
                if current_diagnostics.contains(&warning_str) {
                    continue;
                }

                tracing::debug!(%warning, "Writing tracked warning");
                writer
                    .write_all(warning_str.as_bytes())
                    .await?;
            }
        }

        // Flush and shutdown the writer
        writer.shutdown().await?;

        Ok(())
    }
}

/// How a [`Ghci`] session responds to a reload event.
#[derive(Debug)]
pub enum GhciReloadKind {
    /// Noop. No actions needed.
    None,
    /// Reload, add, and/or remove modules. Can be interrupted.
    Reload,
    /// Restart the whole session. Cannot be interrupted.
    Restart,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ghci::parse::{
        CompilationSummary, CompilingModule, GhcDiagnostic, ModulesLoaded, PositionRange, Severity,
    };
    use camino::Utf8PathBuf;
    use std::collections::BTreeMap;

    /// Helper to create a test `GhcDiagnostic` with the given severity and path.
    fn make_diagnostic(severity: Severity, path: &str, message: &str) -> GhcDiagnostic {
        GhcDiagnostic {
            severity,
            path: Some(Utf8PathBuf::from(path)),
            span: PositionRange::new(1, 1, 1, 1),
            message: message.to_string(),
        }
    }

    /// Helper to create a test `CompilingModule`.
    fn make_compiling_module(name: &str, path: &str) -> CompilingModule {
        CompilingModule {
            name: name.to_string(),
            path: Utf8PathBuf::from(path),
        }
    }

    /// Helper to create a `CompilationLog` with the given modules and diagnostics.
    fn make_compilation_log(
        modules: Vec<CompilingModule>,
        diagnostics: Vec<GhcDiagnostic>,
        result: CompilationResult,
    ) -> CompilationLog {
        CompilationLog {
            summary: Some(CompilationSummary {
                result,
                modules_loaded: ModulesLoaded::Count(modules.len()),
            }),
            diagnostics,
            compiled_modules: modules,
        }
    }

    #[tokio::test]
    async fn test_warning_tracking_basic() {
        // Test the core warning tracking logic using simplified path handling
        let base_dir = std::env::current_dir().unwrap();
        let mut warnings: BTreeMap<NormalPath, Vec<GhcDiagnostic>> = BTreeMap::new();

        // Simulate the first compilation: file A has warnings, file B is clean
        let log1 = make_compilation_log(
            vec![
                make_compiling_module("MyLib", "src/MyLib.hs"),
                make_compiling_module("MyModule", "src/MyModule.hs"),
            ],
            vec![
                make_diagnostic(Severity::Warning, "src/MyLib.hs", "Unused import"),
                make_diagnostic(Severity::Warning, "src/MyLib.hs", "Unused variable"),
                make_diagnostic(Severity::Error, "src/MyModule.hs", "Type error"),
            ],
            CompilationResult::Err,
        );

        // Extract warnings (simulating update_warnings_from_log logic)
        let mut warnings_by_file: BTreeMap<Utf8PathBuf, Vec<GhcDiagnostic>> = BTreeMap::new();
        for diagnostic in &log1.diagnostics {
            if diagnostic.severity == Severity::Warning {
                if let Some(path) = &diagnostic.path {
                    warnings_by_file
                        .entry(path.clone())
                        .or_default()
                        .push(diagnostic.clone());
                }
            }
        }

        // Update warnings for compiled files
        for module in &log1.compiled_modules {
            let path = NormalPath::new(&module.path, &base_dir).unwrap();
            if let Some(file_warnings) = warnings_by_file.remove(&module.path) {
                warnings.insert(path, file_warnings);
            } else {
                warnings.remove(&path);
            }
        }

        // After first compilation: MyLib has 2 warnings, MyModule has 0 warnings
        assert_eq!(warnings.len(), 1);
        let mylib_path = NormalPath::new("src/MyLib.hs", &base_dir).unwrap();
        let mymodule_path = NormalPath::new("src/MyModule.hs", &base_dir).unwrap();
        assert_eq!(warnings.get(&mylib_path).unwrap().len(), 2);
        assert_eq!(warnings.get(&mymodule_path), None);

        // Simulate second compilation: MyLib fixed warnings, MyModule still clean, but only MyLib was recompiled
        let log2 = make_compilation_log(
            vec![make_compiling_module("MyLib", "src/MyLib.hs")],
            vec![], // No diagnostics - warnings fixed
            CompilationResult::Ok,
        );

        // Update warnings again
        let mut warnings_by_file: BTreeMap<Utf8PathBuf, Vec<GhcDiagnostic>> = BTreeMap::new();
        for diagnostic in &log2.diagnostics {
            if diagnostic.severity == Severity::Warning {
                if let Some(path) = &diagnostic.path {
                    warnings_by_file
                        .entry(path.clone())
                        .or_default()
                        .push(diagnostic.clone());
                }
            }
        }

        for module in &log2.compiled_modules {
            let path = NormalPath::new(&module.path, &base_dir).unwrap();
            if let Some(file_warnings) = warnings_by_file.remove(&module.path) {
                warnings.insert(path, file_warnings);
            } else {
                warnings.remove(&path); // Clear warnings for MyLib
            }
        }

        // After second compilation: MyLib warnings cleared (it was recompiled), MyModule warnings unchanged (not recompiled)
        assert_eq!(warnings.len(), 0);
        assert_eq!(warnings.get(&mylib_path), None);
        assert_eq!(warnings.get(&mymodule_path), None);
    }

    #[tokio::test]
    async fn test_warning_persistence_across_dependency_recompilation() {
        // This test simulates the core use case: warnings should persist when a file
        // is recompiled due to dependencies but the file itself didn't change

        let base_dir = std::env::current_dir().unwrap();
        let mut warnings: BTreeMap<NormalPath, Vec<GhcDiagnostic>> = BTreeMap::new();

        // Initial compilation: A has warnings, B is clean
        let log1 = make_compilation_log(
            vec![
                make_compiling_module("A", "src/A.hs"),
                make_compiling_module("B", "src/B.hs"),
            ],
            vec![make_diagnostic(
                Severity::Warning,
                "src/A.hs",
                "Unused import",
            )],
            CompilationResult::Ok,
        );

        // Process initial warnings
        let mut warnings_by_file: BTreeMap<Utf8PathBuf, Vec<GhcDiagnostic>> = BTreeMap::new();
        for diagnostic in &log1.diagnostics {
            if diagnostic.severity == Severity::Warning {
                if let Some(path) = &diagnostic.path {
                    warnings_by_file
                        .entry(path.clone())
                        .or_default()
                        .push(diagnostic.clone());
                }
            }
        }

        for module in &log1.compiled_modules {
            let path = NormalPath::new(&module.path, &base_dir).unwrap();
            if let Some(file_warnings) = warnings_by_file.remove(&module.path) {
                warnings.insert(path, file_warnings);
            } else {
                warnings.remove(&path);
            }
        }

        // A has warnings, B is clean
        assert_eq!(warnings.len(), 1);
        let a_path = NormalPath::new("src/A.hs", &base_dir).unwrap();
        let b_path = NormalPath::new("src/B.hs", &base_dir).unwrap();
        assert_eq!(warnings.get(&a_path).unwrap().len(), 1);

        // Second compilation: only B is recompiled (due to dependency change), A is not touched
        // This simulates the scenario where A's warnings would disappear in normal GHC output
        let log2 = make_compilation_log(
            vec![make_compiling_module("B", "src/B.hs")],
            vec![], // No new warnings
            CompilationResult::Ok,
        );

        // Process second compilation
        let mut warnings_by_file: BTreeMap<Utf8PathBuf, Vec<GhcDiagnostic>> = BTreeMap::new();
        for diagnostic in &log2.diagnostics {
            if diagnostic.severity == Severity::Warning {
                if let Some(path) = &diagnostic.path {
                    warnings_by_file
                        .entry(path.clone())
                        .or_default()
                        .push(diagnostic.clone());
                }
            }
        }

        for module in &log2.compiled_modules {
            let path = NormalPath::new(&module.path, &base_dir).unwrap();
            if let Some(file_warnings) = warnings_by_file.remove(&module.path) {
                warnings.insert(path, file_warnings);
            } else {
                warnings.remove(&path);
            }
        }

        // CRITICAL: A's warnings should still be there (not recompiled), B should have no warnings
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings.get(&a_path).unwrap().len(), 1);
        assert_eq!(warnings.get(&b_path), None);
    }

    #[test]
    fn test_tracked_warnings_exclude_currently_compiled_files() {
        // Test that tracked warnings don't show duplicates for files that were just compiled

        let base_dir = Utf8PathBuf::from("/tmp/test");

        // Set up initial warnings in memory
        let mut warnings: BTreeMap<NormalPath, Vec<GhcDiagnostic>> = BTreeMap::new();
        let file_a_path = NormalPath::new("src/A.hs", &base_dir).unwrap();
        let file_b_path = NormalPath::new("src/B.hs", &base_dir).unwrap();

        // Both files have warnings tracked
        warnings.insert(
            file_a_path.clone(),
            vec![GhcDiagnostic {
                severity: Severity::Warning,
                path: Some("src/A.hs".into()),
                span: PositionRange::new(1, 1, 1, 1),
                message: "Warning in A".to_string(),
            }],
        );
        warnings.insert(
            file_b_path.clone(),
            vec![GhcDiagnostic {
                severity: Severity::Warning,
                path: Some("src/B.hs".into()),
                span: PositionRange::new(2, 1, 2, 1),
                message: "Warning in B".to_string(),
            }],
        );

        // Create a compilation log where only file A was compiled
        let compilation_log = CompilationLog {
            compiled_modules: vec![CompilingModule {
                name: "A".into(),
                path: "src/A.hs".into(),
            }],
            diagnostics: vec![
                // File A shows its warning in fresh compilation output
                GhcDiagnostic {
                    severity: Severity::Warning,
                    path: Some("src/A.hs".into()),
                    span: PositionRange::new(1, 1, 1, 1),
                    message: "Warning in A".to_string(),
                },
            ],
            summary: Some(CompilationSummary {
                result: CompilationResult::Ok,
                modules_loaded: ModulesLoaded::Count(1),
            }),
        };

        // Simulate filtering: when we display tracked warnings, we should exclude file A
        // because it was just compiled and already showed its warnings
        let compiled_files: HashSet<_> = compilation_log
            .compiled_modules
            .iter()
            .map(|module| module.path.as_path())
            .collect();

        // The logic should match what we do in the actual implementation
        // Compare using relative paths since compilation logs use relative paths
        let should_display_a = !compiled_files.contains(file_a_path.relative());
        let should_display_b = !compiled_files.contains(file_b_path.relative());

        // File A should NOT be displayed (it was just compiled)
        assert!(
            !should_display_a,
            "File A was just compiled, its warnings should not be displayed again"
        );

        // File B SHOULD be displayed (it was not compiled, so its warnings are still relevant)
        assert!(
            should_display_b,
            "File B was not compiled, its warnings should still be displayed"
        );
    }

    #[test]
    fn test_error_log_content_generation() {
        // Test the logic for combining current diagnostics with tracked warnings
        use crate::ghci::parse::Severity;
        use std::collections::HashSet;

        // Mock tracked warnings
        let mut warnings: BTreeMap<NormalPath, Vec<GhcDiagnostic>> = BTreeMap::new();
        let base_dir = std::env::current_dir().unwrap();
        let file_a_path = NormalPath::new("src/A.hs", &base_dir).unwrap();
        let file_b_path = NormalPath::new("src/B.hs", &base_dir).unwrap();

        // File A has a tracked warning
        warnings.insert(
            file_a_path.clone(),
            vec![GhcDiagnostic {
                severity: Severity::Warning,
                path: Some("src/A.hs".into()),
                span: PositionRange::new(1, 1, 1, 1),
                message: "Unused import".to_string(),
            }],
        );

        // File B has a tracked error (should not be included in error log)
        warnings.insert(
            file_b_path.clone(),
            vec![GhcDiagnostic {
                severity: Severity::Error,
                path: Some("src/B.hs".into()),
                span: PositionRange::new(2, 1, 2, 1),
                message: "Type error".to_string(),
            }],
        );

        // Create a compilation log with current compilation diagnostics
        let compilation_log = CompilationLog {
            compiled_modules: vec![CompilingModule {
                name: "C".into(),
                path: "src/C.hs".into(),
            }],
            diagnostics: vec![GhcDiagnostic {
                severity: Severity::Warning,
                path: Some("src/C.hs".into()),
                span: PositionRange::new(3, 1, 3, 1),
                message: "Current warning".to_string(),
            }],
            summary: Some(CompilationSummary {
                result: CompilationResult::Ok,
                modules_loaded: ModulesLoaded::Count(1),
            }),
        };

        // Test the logic for combining diagnostics
        let mut content = String::new();

        // Add compilation summary
        if let Some(summary) = compilation_log.summary {
            if let CompilationResult::Ok = summary.result {
                let modules_loaded = if summary.modules_loaded != ModulesLoaded::Count(1) {
                    format!("{} modules", summary.modules_loaded)
                } else {
                    format!("{} module", summary.modules_loaded)
                };
                content.push_str(&format!("All good ({modules_loaded})\n"));
            }
        }

        // Add current compilation diagnostics
        for diagnostic in &compilation_log.diagnostics {
            content.push_str(&diagnostic.to_string());
        }

        // Create deduplication set
        let mut current_diagnostics: HashSet<String> = HashSet::new();
        for diagnostic in &compilation_log.diagnostics {
            current_diagnostics.insert(diagnostic.to_string());
        }

        // Add tracked warnings (only warnings, not errors)
        for file_warnings in warnings.values() {
            for warning in file_warnings {
                // Only include warnings, not errors
                if warning.severity != Severity::Warning {
                    continue;
                }

                let warning_str = warning.to_string();

                // Skip if already in current compilation log
                if current_diagnostics.contains(&warning_str) {
                    continue;
                }

                content.push_str(&warning_str);
            }
        }

        // Verify the content
        assert!(content.contains("All good (1 module)"));
        assert!(content.contains("src/C.hs:3:1: warning: Current warning"));
        assert!(content.contains("src/A.hs:1:1: warning: Unused import"));
        assert!(!content.contains("src/B.hs")); // Error should not be included
        assert!(!content.contains("Type error")); // Error should not be included

        // Verify that errors are filtered out
        let warning_count = warnings
            .values()
            .flatten()
            .filter(|diag| diag.severity == Severity::Warning)
            .count();
        assert_eq!(warning_count, 1); // Only the warning from A.hs should be counted
    }

    #[tokio::test]
    async fn test_write_error_log_with_tracked_warnings() {
        // Test the write_error_log_with_tracked_warnings method by simulating its core logic
        use crate::ghci::parse::Severity;
        use std::collections::HashSet;
        use std::fs;
        use tokio::io::AsyncWriteExt;

        // Create a temporary file for the error log
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("test_error_log.txt");
        let error_log_path = camino::Utf8PathBuf::from_path_buf(temp_file).unwrap();

        // Create mock tracked warnings
        let mut warnings: BTreeMap<NormalPath, Vec<GhcDiagnostic>> = BTreeMap::new();
        let base_dir = std::env::current_dir().unwrap();
        let file_a_path = NormalPath::new("src/A.hs", &base_dir).unwrap();
        let file_b_path = NormalPath::new("src/B.hs", &base_dir).unwrap();

        // Add tracked warnings
        warnings.insert(
            file_a_path.clone(),
            vec![GhcDiagnostic {
                severity: Severity::Warning,
                path: Some("src/A.hs".into()),
                span: PositionRange::new(10, 5, 10, 15),
                message: "Unused import warning".to_string(),
            }],
        );

        warnings.insert(
            file_b_path.clone(),
            vec![GhcDiagnostic {
                severity: Severity::Warning,
                path: Some("src/B.hs".into()),
                span: PositionRange::new(20, 1, 20, 10),
                message: "Unused variable warning".to_string(),
            }],
        );

        // Create a CompilationLog with some current diagnostics
        let compilation_log = CompilationLog {
            summary: Some(CompilationSummary {
                result: CompilationResult::Ok,
                modules_loaded: ModulesLoaded::Count(2),
            }),
            diagnostics: vec![
                GhcDiagnostic {
                    severity: Severity::Warning,
                    path: Some("src/C.hs".into()),
                    span: PositionRange::new(30, 1, 30, 5),
                    message: "Current compilation warning".to_string(),
                },
                GhcDiagnostic {
                    severity: Severity::Error,
                    path: Some("src/C.hs".into()),
                    span: PositionRange::new(31, 1, 31, 5),
                    message: "Current compilation error".to_string(),
                },
            ],
            compiled_modules: vec![CompilingModule {
                name: "C".to_string(),
                path: "src/C.hs".into(),
            }],
        };

        // Simulate the write_error_log_with_tracked_warnings method logic
        let file = tokio::fs::File::create(&error_log_path).await.unwrap();
        let mut writer = tokio::io::BufWriter::new(file);

        // Write compilation summary header if compilation succeeded
        if let Some(summary) = compilation_log.summary {
            if let CompilationResult::Ok = summary.result {
                let modules_loaded = if summary.modules_loaded != ModulesLoaded::Count(1) {
                    format!("{} modules", summary.modules_loaded)
                } else {
                    format!("{} module", summary.modules_loaded)
                };
                writer
                    .write_all(format!("All good ({modules_loaded})\n").as_bytes())
                    .await
                    .unwrap();
            }
        }

        // Write current compilation diagnostics
        for diagnostic in &compilation_log.diagnostics {
            writer
                .write_all(diagnostic.to_string().as_bytes())
                .await
                .unwrap();
        }

        // Create a set of diagnostics from current compilation to avoid duplicates
        let mut current_diagnostics: HashSet<String> = HashSet::new();
        for diagnostic in &compilation_log.diagnostics {
            current_diagnostics.insert(diagnostic.to_string());
        }

        // Write tracked warnings (only warnings, not errors) that are not already in current compilation
        for file_warnings in warnings.values() {
            for warning in file_warnings {
                // Only include warnings, not errors
                if warning.severity != Severity::Warning {
                    continue;
                }

                let warning_str = warning.to_string();

                // Skip if this warning is already in the current compilation log
                if current_diagnostics.contains(&warning_str) {
                    continue;
                }

                writer.write_all(warning_str.as_bytes()).await.unwrap();
            }
        }

        // Flush and shutdown the writer
        writer.shutdown().await.unwrap();

        // Read the content from the error log file
        let content = fs::read_to_string(&error_log_path).unwrap();

        // Verify the output contains expected elements
        // 1. Compilation summary header
        assert!(
            content.contains("All good (2 modules)"),
            "Should contain compilation summary"
        );

        // 2. Current compilation diagnostics (both warning and error)
        assert!(
            content.contains("src/C.hs:30:1-5: warning: Current compilation warning"),
            "Should contain current warning"
        );
        assert!(
            content.contains("src/C.hs:31:1-5: error: Current compilation error"),
            "Should contain current error"
        );

        // 3. Tracked warnings (only warnings, not errors)
        assert!(
            content.contains("src/A.hs:10:5-15: warning: Unused import warning"),
            "Should contain tracked warning from A.hs"
        );
        assert!(
            content.contains("src/B.hs:20:1-10: warning: Unused variable warning"),
            "Should contain tracked warning from B.hs"
        );

        // Test deduplication: create a new compilation log with a duplicate warning
        let compilation_log_with_duplicate = CompilationLog {
            summary: Some(CompilationSummary {
                result: CompilationResult::Ok,
                modules_loaded: ModulesLoaded::Count(1),
            }),
            diagnostics: vec![
                // Same warning as in tracked warnings
                GhcDiagnostic {
                    severity: Severity::Warning,
                    path: Some("src/A.hs".into()),
                    span: PositionRange::new(10, 5, 10, 15),
                    message: "Unused import warning".to_string(),
                },
            ],
            compiled_modules: vec![CompilingModule {
                name: "A".to_string(),
                path: "src/A.hs".into(),
            }],
        };

        // Clear the file and test again
        let file = tokio::fs::File::create(&error_log_path).await.unwrap();
        let mut writer = tokio::io::BufWriter::new(file);

        // Write compilation summary header
        if let Some(summary) = compilation_log_with_duplicate.summary {
            if let CompilationResult::Ok = summary.result {
                let modules_loaded = if summary.modules_loaded != ModulesLoaded::Count(1) {
                    format!("{} modules", summary.modules_loaded)
                } else {
                    format!("{} module", summary.modules_loaded)
                };
                writer
                    .write_all(format!("All good ({modules_loaded})\n").as_bytes())
                    .await
                    .unwrap();
            }
        }

        // Write current compilation diagnostics
        for diagnostic in &compilation_log_with_duplicate.diagnostics {
            writer
                .write_all(diagnostic.to_string().as_bytes())
                .await
                .unwrap();
        }

        // Create deduplication set
        let mut current_diagnostics: HashSet<String> = HashSet::new();
        for diagnostic in &compilation_log_with_duplicate.diagnostics {
            current_diagnostics.insert(diagnostic.to_string());
        }

        // Write tracked warnings (only warnings, not errors) that are not already in current compilation
        for file_warnings in warnings.values() {
            for warning in file_warnings {
                // Only include warnings, not errors
                if warning.severity != Severity::Warning {
                    continue;
                }

                let warning_str = warning.to_string();

                // Skip if this warning is already in the current compilation log
                if current_diagnostics.contains(&warning_str) {
                    continue;
                }

                writer.write_all(warning_str.as_bytes()).await.unwrap();
            }
        }

        writer.shutdown().await.unwrap();

        let content_with_duplicate = fs::read_to_string(&error_log_path).unwrap();

        // Count occurrences of the warning - should appear only once
        let warning_count = content_with_duplicate
            .matches("src/A.hs:10:5-15: warning: Unused import warning")
            .count();
        assert_eq!(
            warning_count, 1,
            "Duplicate warning should only appear once"
        );

        // But the warning from B.hs should still be there since it's not a duplicate
        assert!(
            content_with_duplicate.contains("src/B.hs:20:1-10: warning: Unused variable warning"),
            "Non-duplicate tracked warning should still appear"
        );

        // Clean up
        let _ = fs::remove_file(&error_log_path);
    }
}
