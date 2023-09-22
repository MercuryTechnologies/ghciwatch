//! Graceful shutdown support.

use std::error::Error;
use std::fmt::Display;
use std::future::Future;
use std::ops::DerefMut;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use miette::miette;
use miette::IntoDiagnostic;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tokio::task::AbortHandle;
use tokio::task::JoinHandle;

use crate::format_bulleted_list::format_bulleted_list;

/// A manager for shutting down the program gracefully.
pub struct ShutdownManager {
    /// Sender for shutdown events. Notifies different parts of the program that it's time to shut
    /// down.
    sender: broadcast::Sender<()>,
    // Receiver for shutdown events.
    //
    // The shutdown process begins when this receives a value.
    receiver: broadcast::Receiver<()>,
    /// Shutdown timeout. If the shutdown takes longer than this, we start cancelling tasks.
    timeout: Duration,
    /// The tasks being run.
    handles: Handles,
    /// Shutdown guard. Senders are passed to each future spawned with [`ShutdownManager::spawn`]
    /// so that they're dropped when the task completes. Then, when there are no remaining senders,
    /// the channel is closed and the `guard_receiver` errors, indicating that all tasks have
    /// completed.
    ///
    /// See: <https://tokio.rs/tokio/topics/shutdown#waiting-for-things-to-finish-shutting-down>
    guard_sender: mpsc::Sender<()>,
    /// Shutdown guard receiver.
    guard_receiver: mpsc::Receiver<()>,
}

impl ShutdownManager {
    /// Construct a new shutdown manager with the given timeout for graceful shutdowns.
    pub fn with_timeout(timeout: Duration) -> Self {
        let (sender, receiver) = broadcast::channel(4);
        let (guard_sender, guard_receiver) = mpsc::channel(1);
        Self {
            timeout,
            sender,
            receiver,
            handles: Default::default(),
            guard_receiver,
            guard_sender,
        }
    }

    /// Run a new task in this manager.
    pub async fn spawn<F, Fut>(&mut self, name: String, make_task: F)
    where
        F: FnOnce(ShutdownHandle) -> Fut,
        Fut: Future<Output = miette::Result<()>> + Send + 'static,
    {
        let sender = self.sender.clone();
        let receiver = sender.subscribe();
        // wrap the future?
        // -request shutdowns when tasks fail
        // -cancel tasks from here
        // -check if finished
        // -check if failed later
        let handle = tokio::task::spawn(make_task(ShutdownHandle {
            sender,
            receiver,
            guard: self.guard_sender.clone(),
            handles: self.handles.clone(),
        }));
        self.handles
            .push(Task::new(name, handle, self.sender.clone()))
            .await;
    }

    /// Wait for tasks to shut down/error or Ctrl-C to be pressed and then shuts down gracefully.
    pub async fn wait_for_shutdown(mut self) -> miette::Result<()> {
        drop(self.guard_sender);
        let mut all_finished = false;

        // Wait for a shutdown to be requested or the tasks to finish.
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::debug!("Ctrl-C pressed, shutting down gracefully");
                // Note that we need to trigger the shutdown manually in this case.
                self.sender.send(()).into_diagnostic()?;
            }
            _ = self.guard_receiver.recv() => {
                tracing::debug!("All tasks finished");
                all_finished = true;
            }
            _ = self.receiver.recv() => {
                tracing::debug!("Shutdown requested");
            }
        }

        // If we still have running tasks, begin the graceful shutdown procedure.
        let start_instant = Instant::now();
        if !all_finished {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    tracing::debug!("Ctrl-C pressed again, shutting down immediately");
                }
                _ = self.guard_receiver.recv() => {
                    tracing::debug!("All tasks finished");
                }
                _ = tokio::time::sleep(self.timeout) => {
                    tracing::debug!("Graceful shutdown timed out");
                }
            }
        }
        // Note any unfinished tasks, cancel everything, and check the return values.
        self.handles.cancel_tasks().await;
        let ret = self.handles.check_task_failures().await;

        tracing::debug!("Finished shutdown in {:.2?}", start_instant.elapsed());
        ret
    }
}

/// A set of tasks being run.
#[derive(Debug, Clone, Default)]
struct Handles(Arc<Mutex<Vec<Task>>>);

impl Handles {
    async fn push(&mut self, task: Task) {
        self.0.lock().await.push(task);
    }

    async fn cancel_tasks(&self) {
        for task in self.0.lock().await.iter() {
            if !task.is_finished() {
                tracing::debug!(task = task.name, "Task is unfinished");
            }
            task.cancel();
        }
    }

    async fn check_task_failures(&mut self) -> miette::Result<()> {
        let mut failures = Vec::new();

        for task in std::mem::take(self.0.lock().await.deref_mut()) {
            if let Some(err) = task.into_result().await? {
                failures.push(err);
            }
        }

        if failures.is_empty() {
            tracing::debug!("All tasks completed successfully");
            Ok(())
        } else {
            let failures = format_bulleted_list(
                failures
                    .into_iter()
                    .map(|(name, error)| format!("{name}: {error}")),
            );
            Err(miette!("Tasks failed:\n{failures}"))
        }
    }
}

/// A handle to the shutdown system.
#[derive(Debug)]
pub struct ShutdownHandle {
    /// Sender to request a shutdown.
    sender: broadcast::Sender<()>,
    /// Receiver to be notified of shutdowns.
    receiver: broadcast::Receiver<()>,
    /// Guard for task completion.
    ///
    /// See: <https://tokio.rs/tokio/topics/shutdown#waiting-for-things-to-finish-shutting-down>
    guard: mpsc::Sender<()>,
    /// The tasks being run.
    handles: Handles,
}

impl Clone for ShutdownHandle {
    fn clone(&self) -> Self {
        let sender = self.sender.clone();
        let receiver = sender.subscribe();
        Self {
            sender,
            receiver,
            guard: self.guard.clone(),
            handles: self.handles.clone(),
        }
    }
}

impl ShutdownHandle {
    /// Wait until a shutdown is requested.
    pub async fn on_shutdown_requested(&mut self) -> Result<(), broadcast::error::RecvError> {
        self.receiver.recv().await
    }

    /// Check if a shutdown has been requested; if so, return a [`ShutdownError`].
    ///
    /// Otherwise, return `Ok(())`.
    pub fn error_if_shutdown_requested(&mut self) -> miette::Result<()> {
        match self.receiver.try_recv() {
            Ok(()) | Err(broadcast::error::TryRecvError::Lagged(_)) => Err(ShutdownError.into()),
            Err(broadcast::error::TryRecvError::Empty) => {
                // No shutdown requested.
                Ok(())
            }
            err @ Err(broadcast::error::TryRecvError::Closed) => err.into_diagnostic(),
        }
    }

    /// Request a shutdown.
    pub fn request_shutdown(&self) -> Result<(), broadcast::error::SendError<()>> {
        self.sender.send(()).map(|_| ())
    }

    /// Spawn a new task under this handle.
    pub async fn spawn<F, Fut>(&mut self, name: String, make_task: F)
    where
        F: FnOnce(ShutdownHandle) -> Fut,
        Fut: Future<Output = miette::Result<()>> + Send + 'static,
    {
        let handle = tokio::task::spawn(make_task(self.clone()));
        self.handles
            .push(Task::new(name, handle, self.sender.clone()))
            .await;
    }
}

/// A task being managed by a [`ShutdownManager`].
#[derive(Debug)]
struct Task {
    /// The name of the running task.
    name: String,
    /// A handle for remotely cancelling the task.
    abort_handle: AbortHandle,
    /// A receiver for the task's return value.
    receiver: oneshot::Receiver<Option<miette::Report>>,
    /// A handle for the manager which runs asynchronously and requests a shutdown if the task
    /// errors.
    #[allow(dead_code)]
    manager_handle: JoinHandle<()>,
}

impl Task {
    /// Create a new task with the given name and handle.
    fn new(
        name: String,
        handle: JoinHandle<miette::Result<()>>,
        request_shutdown: broadcast::Sender<()>,
    ) -> Self {
        let abort_handle = handle.abort_handle();
        let (sender, receiver) = oneshot::channel();
        let manager_handle = tokio::task::spawn(manage_handle(
            name.clone(),
            handle,
            request_shutdown,
            sender,
        ));
        Self {
            name,
            abort_handle,
            manager_handle,
            receiver,
        }
    }

    /// Cancel the task.
    fn cancel(&self) {
        self.abort_handle.abort();
    }

    /// Check if the task is finished.
    fn is_finished(&self) -> bool {
        self.abort_handle.is_finished()
    }

    /// Wait for the task to complete and get its name and an error message if it fails.
    async fn into_result(self) -> miette::Result<Option<(String, miette::ErrReport)>> {
        let maybe_error = self.receiver.await.into_diagnostic()?;
        Ok(maybe_error.map(|err| (self.name, err)))
    }
}

/// Manage a task, requesting a shutdown if it fails and notifying the given sender of any errors.
async fn manage_handle(
    name: String,
    handle: JoinHandle<miette::Result<()>>,
    request_shutdown: broadcast::Sender<()>,
    sender: oneshot::Sender<Option<miette::Report>>,
) {
    let mut ret = None;
    match handle.await {
        Ok(Ok(())) => {
            tracing::debug!(task = name, "Task completed successfully");
        }
        Ok(Err(err)) => {
            if err.downcast_ref::<ShutdownError>().is_some() {
                tracing::debug!(task = name, "Task shut down gracefully");
            } else {
                tracing::debug!(task = name, "Task failed: {err:?}");
                ret = Some(err);
            }
        }
        Err(err) => {
            if err.is_cancelled() {
                tracing::debug!(task = name, "Task cancelled");
            } else {
                tracing::debug!(task = name, "Task panicked: {err}");
                ret = Some(miette!("{err}"));
            }
        }
    }
    if ret.is_some() {
        let _ = request_shutdown.send(());
    }
    let _ = sender.send(ret);
}

/// A shutdown was requested.
///
/// This error can be returned to indicate that the task failed to finish its computation due to a
/// shutdown being requested. Unlike other errors, this won't be displayed as a failure in the
/// [`ShutdownManager::wait_for_shutdown`] return value.
#[derive(Debug, Clone, Copy)]
pub struct ShutdownError;

impl ShutdownError {
    /// Get a [`miette::Report`] of a [`ShutdownError`].
    pub fn as_report() -> miette::Report {
        miette::Report::msg(Self)
    }
}

impl Error for ShutdownError {}

impl Display for ShutdownError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Shutdown requested")
    }
}

impl miette::Diagnostic for ShutdownError {}
