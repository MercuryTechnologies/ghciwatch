//! The [`Runner`] struct.

use camino::Utf8Path;
use miette::miette;
use miette::IntoDiagnostic;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::task;
use tokio::task::JoinError;
use tokio::task::JoinHandle;
use tracing::instrument;

use crate::event_filter::FileEvent;
use crate::ghci::Ghci;
use crate::socket::ServerNotification;
use crate::socket::SocketConnector;
use crate::watcher::Watcher;

/// An event sent to a [`Runner`].
#[derive(Debug)]
pub enum RunnerEvent {
    /// File change(s) that `ghci` will need to respond to, typically by reloading, restarting, or
    /// adding new modules to the environment.
    FileChange {
        /// The file events.
        events: Vec<FileEvent>,
    },
    /// Quit the `ghci` session and exit `ghcid-ng`.
    Exit,
}

/// The `ghcid-ng` runner, responsible for orchestrating a `ghci` session.
///
/// This runner coordinates between file events from a [`crate::watcher::Watcher`]
/// and (if `ghcid-ng` is running in server mode) server commands from a socket.
///
/// The basic idea is that whereas [`Ghci`] manages a `ghci` session (coordinating its inputs and
/// outputs into discrete actions as described by its methods), this struct manages the [`Ghci`]
/// struct, reading events from multiple sources (via an [`mpsc`] channel) and executing the
/// corresponding [`Ghci`] methods in turn.
///
/// This is mostly nice so that we can avoid having a bunch of server logic in the [`Ghci`] struct.
#[derive(Debug)]
pub struct Runner {
    ghci: Ghci,
    watcher: Watcher,
    receiver: mpsc::Receiver<RunnerEvent>,
    notification_sender: Option<broadcast::Sender<ServerNotification>>,
    /// Socket connector handle. We need to keep this around or the task will be dropped and
    /// cancelled.
    #[allow(dead_code)]
    socket_connector: Option<JoinHandle<miette::Result<()>>>,
}

impl Runner {
    /// Construct a new runner orchestrating the given session with the given file watcher.
    pub fn new(
        sender: mpsc::Sender<RunnerEvent>,
        receiver: mpsc::Receiver<RunnerEvent>,
        ghci: Ghci,
        watcher: Watcher,
        socket: Option<&Utf8Path>,
    ) -> miette::Result<Self> {
        let (socket_connector, notification_sender) = match socket {
            Some(path) => {
                let (notification_sender, notification_receiver) = broadcast::channel(8);
                let handle = task::spawn(
                    SocketConnector::new(path, sender.clone(), notification_receiver)?.run(),
                );
                (Some(handle), Some(notification_sender))
            }
            None => (None, None),
        };

        Ok(Self {
            receiver,
            ghci,
            watcher,
            notification_sender,
            socket_connector,
        })
    }

    /// Run the runner! This watches for events in this runner's receiver channel and responds
    /// appropriately, e.g. by reloading `ghci`.
    #[instrument(skip_all, name = "runner", level = "debug")]
    pub async fn run(mut self) -> Result<miette::Result<()>, JoinError> {
        // This assignment needs to be up here to avoid a borrow error.
        let watcher_handle = self.watcher.run();

        tokio::select! {
            dispatch_result = self.dispatch_loop() => {
                Ok(dispatch_result)
            }
            watcher_result = watcher_handle => {
                self.exit().await;
                watcher_result.map(|res| res.into_diagnostic())
            }
        }
    }

    async fn dispatch_loop(&mut self) -> miette::Result<()> {
        while let Some(event) = self.receiver.recv().await {
            match self.dispatch(event).await {
                Ok(ShouldExit::Exit) => {
                    return Ok(());
                }
                Ok(ShouldExit::Continue) => {}
                Err(err) => {
                    tracing::error!("{err:?}");
                }
            }
        }

        Err(miette!("Runner channel closed"))
    }

    async fn dispatch(&mut self, event: RunnerEvent) -> miette::Result<ShouldExit> {
        match event {
            RunnerEvent::FileChange { events } => {
                self.file_change(events).await?;
            }
            RunnerEvent::Exit => {
                self.exit().await;
                return Ok(ShouldExit::Exit);
            }
        }

        Ok(ShouldExit::Continue)
    }

    #[instrument(skip(self), level = "debug")]
    async fn file_change(&mut self, events: Vec<FileEvent>) -> miette::Result<()> {
        self.notify(ServerNotification::Reload).await;
        self.ghci.reload(events).await?;
        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    async fn exit(&mut self) {
        tracing::info!("Exiting");
        self.notify(ServerNotification::Exit).await;
    }

    #[instrument(skip(self), level = "debug")]
    async fn notify(&mut self, notification: ServerNotification) {
        if let Some(sender) = &self.notification_sender {
            match sender.send(notification) {
                Ok(subscribed_handles) => {
                    tracing::debug!("Sent notification to {subscribed_handles} handles");
                }
                Err(err) => {
                    tracing::error!("{err:?}");
                }
            }
        }
    }
}

/// Should `ghcid-ng` exit?
enum ShouldExit {
    /// Yes, exit now.
    Exit,
    /// No, continue running and waiting for events.
    Continue,
}
