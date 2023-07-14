//! The core [`Ghci`] session struct.

use std::fmt::Debug;
use std::fmt::Display;
use std::process::Stdio;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use camino::Utf8PathBuf;
use itertools::Itertools;
use miette::IntoDiagnostic;
use miette::WrapErr;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::Child;
use tokio::process::ChildStderr;
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

mod show_modules;
use show_modules::ModuleSet;

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
    /// A channel for sending events to interact with the stdout task.
    stdout_channel: mpsc::Sender<StdoutEvent>,
    /// Count of 'sync' events sent. This lets us sync stdin/stdout -- we write a message to stdin
    /// instructing `ghci` to print a sentinel string, and wait to read that string on `stdout`.
    sync_count: AtomicUsize,
    /// The currently-loaded modules in this `ghci` session.
    modules: ModuleSet,
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
    pub async fn new(command_arc: Arc<Mutex<Command>>) -> miette::Result<Arc<Mutex<Self>>> {
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
        let stderr = BufReader::new(child.stderr.take().unwrap());

        // TODO: Is this a good capacity? Maybe it should just be 1.
        let (stdin_sender, stdin_receiver) = mpsc::channel(8);
        let (stdout_sender, stdout_receiver) = mpsc::channel(8);

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
            stdout_channel: stdout_sender.clone(),
            sync_count: AtomicUsize::new(0),
            modules: Default::default(),
        }));

        let (init_sender, init_receiver) = oneshot::channel::<()>();

        // Three tasks for my three beautiful streams.
        let stdout = task::spawn(
            GhciStdout {
                ghci: Arc::downgrade(&ret),
                reader: IncrementalReader::new(stdout).with_writer(tokio::io::stdout()),
                stdin_sender: stdin_sender.clone(),
                receiver: stdout_receiver,
                buffer: vec![0; LINE_BUFFER_CAPACITY],
            }
            .run(init_sender),
        );
        let stderr = task::spawn(stderr_task(stderr));
        let stdin = task::spawn(
            GhciStdin {
                ghci: Arc::downgrade(&ret),
                stdin,
                stdout_sender,
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
        init_receiver.await.into_diagnostic()?;

        let (initialize_event, init_receiver) = StdinEvent::initialize();

        // Perform start-of-session initialization.
        stdin_sender
            .send(initialize_event)
            .await
            .into_diagnostic()?;

        init_receiver.await.into_diagnostic()?;

        {
            // Sync up for any prompts.
            let mut guard = ret.lock().await;
            guard.sync().await?;
            // Get the initial list of loaded modules.
            guard.refresh_modules().await?;
        }

        tracing::info!("`ghci` ready!");
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
                            // Otherwise we need to `:load` the new paths.
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
            return Self::new(command).await;
        }

        if !add.is_empty() {
            tracing::info!(
                "Adding new modules to `ghci`:\n{}",
                format_bulleted_list(&add)
            );
            for path in add {
                this.lock().await.add_module(path).await?;
            }
        }

        if !needs_reload.is_empty() {
            tracing::info!(
                "Reloading `ghci` due to changed modules:\n{}",
                format_bulleted_list(&needs_reload)
            );
            let (sender, receiver) = oneshot::channel();
            this.lock()
                .await
                .stdin_channel
                .send(StdinEvent::Reload(sender))
                .await
                .into_diagnostic()?;
            receiver.await.into_diagnostic()?;
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

    /// `:load` a module to the `ghci` session by path.
    #[instrument(skip(self), level = "debug")]
    pub async fn add_module(&mut self, path: Utf8PathBuf) -> miette::Result<()> {
        let (sender, receiver) = oneshot::channel();
        self.stdin_channel
            .send(StdinEvent::AddModule(path.clone(), sender))
            .await
            .into_diagnostic()?;
        // TODO: What if adding the new module fails?
        self.modules.insert_source_path(path);
        receiver.await.into_diagnostic()?;
        Ok(())
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

#[instrument(skip_all, level = "debug")]
async fn stderr_task(stderr: BufReader<ChildStderr>) -> miette::Result<()> {
    let mut lines = stderr.lines();
    while let Some(line) = lines.next_line().await.into_diagnostic()? {
        tracing::info!("[ghci stderr] {line}");
    }

    Ok(())
}

fn format_bulleted_list(items: &[impl Display]) -> String {
    if items.is_empty() {
        String::new()
    } else {
        format!("• {}", items.iter().join("\n• "))
    }
}
