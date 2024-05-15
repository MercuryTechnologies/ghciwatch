use std::time::Duration;

use miette::miette;
use miette::IntoDiagnostic;
use notify_debouncer_full::notify;
use notify_debouncer_full::notify::PollWatcher;
use notify_debouncer_full::notify::RecommendedWatcher;
use notify_debouncer_full::notify::RecursiveMode;
use notify_debouncer_full::DebounceEventHandler;
use notify_debouncer_full::DebounceEventResult;
use notify_debouncer_full::Debouncer;
use notify_debouncer_full::FileIdMap;
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::task::block_in_place;
use tracing::instrument;

use crate::cli::Opts;
use crate::event_filter::file_events_from_action;
use crate::ghci::manager::GhciEvent;
use crate::normal_path::NormalPath;
use crate::shutdown::ShutdownHandle;

/// Options for [`run_watcher`]. This is like a lower-effort builder interface, mostly
/// provided because Rust tragically lacks named arguments.
pub struct WatcherOpts {
    /// The paths to watch for changes.
    pub watch: Vec<NormalPath>,
    /// Debounce duration for filesystem events.
    pub debounce: Duration,
    /// If given, use the polling file watcher with the given duration as the poll interval.
    pub poll: Option<Duration>,
}

impl WatcherOpts {
    /// Construct options for [`run_watcher`] from parsed command-line interface arguments as [`Opts`].
    ///
    /// This extracts the bits of an [`Opts`] struct relevant to the [`run_watcher`] session
    /// without cloning or taking ownership of the entire thing.
    pub fn from_cli(opts: &Opts) -> Self {
        let watch = if let Some(file) = &opts.file {
            let mut paths = opts.watch.paths.clone();
            paths.push(file.clone());
            paths
        } else {
            opts.watch.paths.clone()
        };

        Self {
            watch,
            debounce: opts.watch.debounce,
            poll: opts.watch.poll,
        }
    }
}

/// A [`notify`] watcher which waits for file changes and sends reload events to the contained
/// `ghci` session.
#[instrument(level = "debug", skip_all)]
pub async fn run_watcher(
    handle: ShutdownHandle,
    ghci_sender: mpsc::Sender<GhciEvent>,
    opts: WatcherOpts,
) -> miette::Result<()> {
    if opts.poll.is_some() {
        run_debouncer::<PollWatcher>(handle, ghci_sender, opts).await
    } else {
        run_debouncer::<RecommendedWatcher>(handle, ghci_sender, opts).await
    }
}

async fn run_debouncer<T: notify::Watcher>(
    mut handle: ShutdownHandle,
    ghci_sender: mpsc::Sender<GhciEvent>,
    opts: WatcherOpts,
) -> miette::Result<()> {
    let mut config = notify::Config::default();
    if let Some(interval) = opts.poll {
        config = config.with_poll_interval(interval);
    }

    let event_handler = EventHandler {
        handle: Handle::current(),
        ghci_sender,
        shutdown: handle.clone(),
    };

    let cache = FileIdMap::new();

    // `tick_rate` defaults to 1/4 of the debounce duration.
    let tick_rate = None;

    let mut debouncer: Debouncer<T, FileIdMap> = notify_debouncer_full::new_debouncer_opt(
        opts.debounce,
        tick_rate,
        event_handler,
        cache,
        config,
    )
    .into_diagnostic()?;

    {
        let watcher = debouncer.watcher();
        for path in &opts.watch {
            watcher
                .watch(path.as_std_path(), RecursiveMode::Recursive)
                .into_diagnostic()?;
        }
        let mut cache = debouncer.cache();
        for path in &opts.watch {
            cache.add_root(path.as_std_path(), RecursiveMode::Recursive);
        }
    }

    tracing::debug!("notify watcher started");

    // Wait for a shutdown request, either from another subsystem or from an error in the handler.
    let _ = handle.on_shutdown_requested().await;

    block_in_place(|| debouncer.stop());

    Ok(())
}

struct EventHandler {
    handle: Handle,
    ghci_sender: mpsc::Sender<GhciEvent>,
    shutdown: ShutdownHandle,
}

impl EventHandler {
    async fn handle_event_async(&self, event: DebounceEventResult) {
        if let Err(err) = self.handle_event_inner(event).await {
            tracing::error!("{err:?}");
            let _ = self.shutdown.request_shutdown();
        }
    }

    #[instrument(skip_all, level = "debug")]
    async fn handle_event_inner(&self, event: DebounceEventResult) -> miette::Result<()> {
        let events = match event {
            Ok(events) => events,
            Err(errors) => {
                for err in errors {
                    tracing::error!("{err}");
                }
                return Err(miette!("Watching files failed"));
            }
        };

        tracing::trace!(?events, "Got events");

        // TODO: On Linux, sometimes we get a "new directory" event but none of the events for
        // files inside of it. When we get new directories, we should paw through them with
        // `walkdir` or something to check for files.
        let events = file_events_from_action(events)?;
        if events.is_empty() {
            tracing::debug!("No relevant file events");
        } else {
            tracing::trace!(?events, "Processed events");
            self.ghci_sender
                .send(GhciEvent::Reload { events })
                .await
                .into_diagnostic()?;
        }

        Ok(())
    }
}

impl DebounceEventHandler for EventHandler {
    fn handle_event(&mut self, event: DebounceEventResult) {
        self.handle.block_on(self.handle_event_async(event))
    }
}
