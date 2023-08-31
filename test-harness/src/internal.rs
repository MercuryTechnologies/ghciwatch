//! Internal functions, exposed for the `#[test]` attribute macro.

use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Duration;

use miette::miette;
use miette::Context;
use miette::IntoDiagnostic;
use nix::sys::signal;
use nix::sys::signal::Signal;
use nix::unistd::Pid;
use tokio::process::Child;

thread_local! {
    /// The temporary directory where `ghcid-ng` is run. Note that because tests are run with the
    /// `tokio` current-thread runtime, this is unique per-test.
    pub(crate) static TEMPDIR: RefCell<Option<PathBuf>> = RefCell::new(None);

    /// The GHC version to use for this test. This should be a string like `ghc962`.
    /// This is used to open a corresponding (e.g.) `nix develop .#ghc962` shell to run `ghcid-ng`
    /// in.
    pub static GHC_VERSION: RefCell<String> = RefCell::new(String::new());

    /// The GHC process for this test.
    ///
    /// This is set so that we can make sure to kill it when the test ends.
    pub(crate) static GHC_PROCESS: RefCell<Option<Child>> = RefCell::new(None);
}

/// Save the test logs in `TEMPDIR` to `cargo_target_tmpdir`.
///
/// This is called when a `#[test]`-annotated function panics, to persist the logs for further
/// analysis.
pub fn save_test_logs(test_name: String, cargo_target_tmpdir: PathBuf) {
    let log_path: PathBuf = TEMPDIR.with(|tempdir| {
        tempdir
            .borrow()
            .as_deref()
            .map(|path| path.join(crate::ghcid_ng::LOG_FILENAME))
            .expect("`test_harness::TEMPDIR` is not set")
    });

    let test_name = test_name.replace("::", "-");
    let persist_log_path = cargo_target_tmpdir.join(format!("{test_name}.json"));
    if persist_log_path.exists() {
        // Cargo doesn't manage `CARGO_TARGET_TMPDIR` for us, so we remove output from old tests
        // ourself.
        std::fs::remove_file(&persist_log_path).expect("Failed to remove log output");
    }

    if !log_path.exists() {
        eprintln!("No logs were written");
    } else {
        let logs = std::fs::read_to_string(log_path).expect("Failed to read logs");
        std::fs::write(&persist_log_path, logs).expect("Failed to write logs");
        eprintln!("Wrote logs to {}", persist_log_path.display());
    }
}

/// Perform end-of-test cleanup.
///
/// 1. Kill the [`GHC_PROCESS`].
/// 2. Remove the [`TEMPDIR`] from the filesystem.
pub async fn cleanup() {
    let child = GHC_PROCESS.with(|child| child.take());
    match child {
        None => {
            panic!("`GHC_PROCESS` is not set");
        }
        Some(mut child) => {
            send_signal(&child, Signal::SIGINT).expect("Failed to send SIGINT to `ghcid-ng`");
            match tokio::time::timeout(Duration::from_secs(10), child.wait()).await {
                Err(_) => {
                    tracing::info!("ghcid-ng didn't exit in time, killing");
                    child
                        .kill()
                        .await
                        .expect("Failed to kill `ghcid-ng` after test completion");
                }
                Ok(Ok(status)) => {
                    tracing::info!(?status, "ghcid-ng exited");
                }
                Ok(Err(err)) => {
                    tracing::error!("Waiting for ghcid-ng to exit failed: {err}");
                }
            }
        }
    }

    let path = TEMPDIR.with(|path| path.take());
    match path {
        None => {
            panic!("`TEMPDIR` is not set");
        }
        Some(path) => {
            if let Err(err) = tokio::fs::remove_dir_all(&path).await {
                // Run `find` on the directory so we can see what's in it?
                let _status = tokio::process::Command::new("find")
                    .arg(&path)
                    .status()
                    .await;
                // Try an `rm -rf` for good luck :)
                let _status = tokio::process::Command::new("rm")
                    .arg("-rf")
                    .arg(&path)
                    .status()
                    .await;
                if path.exists() {
                    tracing::error!("Failed to remove TEMPDIR: {path:?}\n{err}");
                } else {
                    tracing::warn!("Failed to remove TEMPDIR with `remove_dir_all`, but `rm -rf` worked: {path:?}\n{err}");
                }
            }
        }
    }
}

/// Get the GHC version as given by [`GHC_VERSION`].
pub(crate) fn get_ghc_version() -> miette::Result<String> {
    let ghc_version = GHC_VERSION.with(|version| version.borrow().to_owned());
    if ghc_version.is_empty() {
        Err(miette!("`GHC_VERSION` is not set"))
            .wrap_err("`GhcidNg` can only be used in `#[test_harness::test]` functions")
    } else {
        Ok(ghc_version)
    }
}

/// Create a new temporary directory and set [`TEMPDIR`] to it, persisting it to disk.
///
/// Fails if [`TEMPDIR`] is already set.
pub(crate) fn set_tempdir() -> miette::Result<PathBuf> {
    let tempdir = tempfile::tempdir()
        .into_diagnostic()
        .wrap_err("Failed to create temporary directory")?;

    // Set the thread-local tempdir for cleanup later.
    TEMPDIR.with(|thread_tempdir| {
        if thread_tempdir.borrow().is_some() {
            return Err(miette!(
                "`GhcidNg` can only be constructed once per `#[test_harness::test]` function"
            ));
        }
        *thread_tempdir.borrow_mut() = Some(tempdir.path().to_path_buf());
        Ok(())
    })?;

    // Now we can persist the tempdir to disk, knowing the test harness will clean it up later.
    Ok(tempdir.into_path())
}

/// Set [`GHC_PROCESS`] to the given [`Child`].
///
/// Fails if [`GHC_PROCESS`] is already set.
pub(crate) fn set_ghc_process(child: Child) -> miette::Result<()> {
    GHC_PROCESS.with(|maybe_child| {
        if maybe_child.borrow().is_some() {
            return Err(miette!(
                "`GhcidNg` can only be constructed once per `#[test_harness::test]` function"
            ));
        }

        *maybe_child.borrow_mut() = Some(child);

        Ok(())
    })
}

/// Send a signal to a child process.
fn send_signal(child: &Child, signal: Signal) -> miette::Result<()> {
    signal::kill(
        Pid::from_raw(
            child
                .id()
                .ok_or_else(|| miette!("Command has no pid, likely because it has already exited"))?
                .try_into()
                .into_diagnostic()
                .wrap_err("Failed to convert pid type")?,
        ),
        signal,
    )
    .into_diagnostic()
}
