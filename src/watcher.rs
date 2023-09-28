//! A [`Watcher`], which waits for file changes and sends reload events to the `ghci` session.

use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::task::block_in_place;
use tokio::task::JoinHandle;
use tracing::instrument;
use watchexec::action::Action;
use watchexec::action::Outcome;
use watchexec::config::InitConfig;
use watchexec::config::RuntimeConfig;
use watchexec::error::RuntimeError;
use watchexec::event::Event;
use watchexec::handler::Handler;
use watchexec::ErrorHook;
use watchexec::Watchexec;
use watchexec_signals::Signal;

use crate::cli::Opts;
use crate::event_filter::file_events_from_action;
use crate::ghci::Ghci;
use crate::normal_path::NormalPath;

/// Options for constructing a [`Watcher`]. This is like a lower-effort builder interface, mostly
/// provided because Rust tragically lacks named arguments.
pub struct WatcherOpts<'opts> {
    /// The paths to watch for changes.
    pub watch: &'opts [NormalPath],
    /// Paths to watch for changes and restart the `ghci` session on.
    pub watch_restart: &'opts [NormalPath],
    /// Debounce duration for filesystem events.
    pub debounce: Duration,
    /// If given, use the polling file watcher with the given duration as the poll interval.
    pub poll: Option<Duration>,
    /// Extra file extensions to reload on.
    pub extra_extensions: &'opts [String],
}

impl<'opts> WatcherOpts<'opts> {
    /// Construct options for [`Watcher`] from parsed command-line interface arguments as [`Opts`].
    ///
    /// This extracts the bits of an [`Opts`] struct relevant to the [`Watcher`] session without
    /// cloning or taking ownership of the entire thing.
    pub fn from_cli(opts: &'opts Opts) -> Self {
        Self {
            watch: &opts.watch.paths,
            watch_restart: &opts.watch.restart_paths,
            debounce: opts.watch.debounce,
            poll: opts.watch.poll,
            extra_extensions: &opts.watch.extensions,
        }
    }
}

/// A [`watchexec`] watcher which waits for file changes and sends reload events to the contained
/// `ghci` session.
pub struct Watcher {
    /// The inner `Watchexec` struct.
    ///
    /// This field isn't read, but it has to be here or the watcher stops working. Dropping this
    /// drops the watcher tasks too.
    #[allow(dead_code)]
    inner: Arc<Watchexec>,
    /// A handle to wait on the file watcher task.
    pub handle: JoinHandle<Result<(), watchexec::error::CriticalError>>,
}

impl Watcher {
    /// Create a new [`Watcher`] from a [`Ghci`] session.
    pub fn new(ghci: Ghci, opts: WatcherOpts) -> miette::Result<Self> {
        let mut init_config = InitConfig::default();
        init_config.on_error(|error_hook: ErrorHook| async move {
            match error_hook.error {
                RuntimeError::Exit => {
                    // Graceful exit.
                }
                RuntimeError::Handler { err, .. } => {
                    // The `RuntimeError` display isn't great for these errors, it prefixes some
                    // nonsense like `handler error while action worker`. Let's just print our
                    // contained error.
                    tracing::error!("{}", err);
                }
                err => {
                    // Some other error.
                    tracing::error!("{}", err);
                }
            }
            Ok::<(), RuntimeError>(())
        });

        let action_handler = ActionHandler { ghci };

        let mut runtime_config = RuntimeConfig::default();
        runtime_config
            .pathset(opts.watch.iter().chain(opts.watch_restart))
            .action_throttle(opts.debounce)
            .on_action(action_handler);

        if let Some(interval) = opts.poll {
            runtime_config.file_watcher(watchexec::fs::Watcher::Poll(interval));
        }

        let watcher = Watchexec::new(init_config, runtime_config.clone())?;

        let watcher_handle = watcher.main();

        Ok(Self {
            inner: watcher,
            handle: watcher_handle,
        })
    }
}

struct ActionHandler {
    ghci: Ghci,
}

impl ActionHandler {
    #[instrument(skip_all, level = "debug")]
    async fn on_action(&mut self, action: Action) -> miette::Result<()> {
        let signals = action
            .events
            .iter()
            .flat_map(Event::signals)
            .collect::<Vec<_>>();

        if signals.iter().any(|sig| sig == &Signal::Interrupt) {
            tracing::debug!("Received SIGINT, exiting.");
            action.outcome(Outcome::Exit);
            return Ok(());
        }

        tracing::trace!(events = ?action.events, "Got events");

        // TODO: On Linux, sometimes we get a "new directory" event but none of the events for
        // files inside of it. When we get new directories, we should paw through them with
        // `walkdir` or something to check for files.
        let events = file_events_from_action(&action)?;
        if events.is_empty() {
            tracing::debug!("No relevant file events");
        } else if let Err(err) = self.ghci.reload(events).await {
            tracing::error!("{err:?}");
            action.outcome(Outcome::Exit);
        }

        Ok(())
    }
}

impl Handler<Action> for ActionHandler {
    fn handle(&mut self, action: Action) -> Result<(), Box<dyn Error>> {
        // This implementation is copied from the `watchexec` `Handler` impl for closures... no
        // clue why I can't get it to work without this -- rustc complains my closure implements
        // `FnOnce`, not `FnMut`.

        // This will always be called within the watchexec context, which runs within tokio
        block_in_place(|| {
            Handle::current()
                .block_on(self.on_action(action))
                // The `as _` here seems to cast from a `MietteDiagnostic` to a `dyn Error`.
                .map_err(|e| Box::new(miette::MietteDiagnostic::new(format!("{e:?}"))) as _)
        })
    }
}
