//! Internal functions, exposed for the `#[test]` attribute macro.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::SeqCst;

use miette::miette;
use miette::Context;
use miette::IntoDiagnostic;

thread_local! {
    /// The temporary directory where `ghcid-ng` is run. Note that because tests are run with the
    /// `tokio` current-thread runtime, this is unique per-test.
    pub static TEMPDIR: RefCell<Option<PathBuf>> = RefCell::new(None);

    /// Directory to put failed test logs in. The `#[test]` attribute sets this at the start of the
    /// test to the value of the compile-time environment variable `$CARGO_TARGET_TMPDIR`.
    /// See: <https://doc.rust-lang.org/cargo/reference/environment-variables.html>
    pub static CARGO_TARGET_TMPDIR: RefCell<Option<PathBuf>> = RefCell::new(None);

    /// The GHC version to use for this test. This should be a string like `ghc962`.
    /// This is used to open a corresponding (e.g.) `nix develop .#ghc962` shell to run `ghcid-ng`
    /// in.
    pub static GHC_VERSION: RefCell<String> = RefCell::new(String::new());

    /// Is this thread running in the custom test harness?
    /// If `GhcidNg::new` was used outside of our custom test harness, the temporary directory
    /// wouldn't be cleaned up -- this lets us detect that case and error to avoid it.
    pub static IN_CUSTOM_TEST_HARNESS: AtomicBool = const { AtomicBool::new(false) };
}

/// Save the test logs in `TEMPDIR` to `CARGO_TARGET_TMPDIR`.
///
/// This is called when a `#[test]`-annotated function panics, to persist the logs for further
/// analysis.
pub fn save_test_logs(test_name: String) {
    let log_path: PathBuf = TEMPDIR.with(|tempdir| {
        tempdir
            .borrow()
            .as_deref()
            .map(|path| path.join(crate::ghcid_ng::LOG_FILENAME))
            .expect("`test_harness::TEMPDIR` is not set")
    });
    let persist_to = CARGO_TARGET_TMPDIR.with(|dir| {
        dir.borrow()
            .clone()
            .expect("`CARGO_TARGET_TMPDIR` is not set")
    });

    let test_name = test_name.replace("::", "-");
    let persist_log_path = persist_to.join(format!("{test_name}.json"));
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

/// Remove the [`TEMPDIR`] from the filesystem. This is called at the end of `#[test]`-annotated
/// functions.
pub fn cleanup_tempdir() {
    TEMPDIR.with(|path| {
        std::fs::remove_dir_all(path.borrow().as_deref().expect("`TEMPDIR` is not set"))
            .expect("Failed to clean up `TEMPDIR`");
    });
}

/// Fail if [`IN_CUSTOM_TEST_HARNESS`] has not been set.
pub(crate) fn ensure_in_custom_test_harness() -> miette::Result<()> {
    if IN_CUSTOM_TEST_HARNESS.with(|value| value.load(SeqCst)) {
        Ok(())
    } else {
        Err(miette!(
            "`GhcidNg` can only be used in `#[test_harness::test]` functions"
        ))
    }
}

/// Get the GHC version as given by [`GHC_VERSION`].
pub(crate) fn get_ghc_version() -> miette::Result<String> {
    let ghc_version = GHC_VERSION.with(|version| version.borrow().to_owned());
    if ghc_version.is_empty() {
        Err(miette!("`GHC_VERSION` should be set"))
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
