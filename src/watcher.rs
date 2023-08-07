//! A [`Watcher`], which waits for file changes and sends reload events to the `ghci` session.

use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use camino::Utf8PathBuf;
use tokio::runtime::Handle;
use tokio::sync::Mutex;
use tokio::task::block_in_place;
use tokio::task::JoinHandle;
use tracing::instrument;
use watchexec::action::Action;
use watchexec::action::Outcome;
use watchexec::config::InitConfig;
use watchexec::config::RuntimeConfig;
use watchexec::event::Event;
use watchexec::handler::Handler;
use watchexec::handler::PrintDebug;
use watchexec::Watchexec;
use watchexec_signals::Signal;

use crate::event_filter::file_events_from_action;
use crate::ghci::Ghci;

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
    pub fn new(
        ghci: Arc<Mutex<Ghci>>,
        watch: &[Utf8PathBuf],
        debounce: Duration,
        poll: Option<Duration>,
    ) -> miette::Result<Self> {
        let mut init_config = InitConfig::default();
        init_config.on_error(PrintDebug(std::io::stderr()));

        let action_handler = ActionHandler { ghci };

        let mut runtime_config = RuntimeConfig::default();
        runtime_config
            .pathset(watch)
            .action_throttle(debounce)
            .on_action(action_handler);

        if let Some(interval) = poll {
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

#[derive(Clone)]
struct ActionHandler {
    ghci: Arc<Mutex<Ghci>>,
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

        let events = file_events_from_action(&action)?;
        if !events.is_empty() {
            self.ghci = Ghci::reload(self.ghci.clone(), events).await?;
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
