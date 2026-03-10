use std::path::PathBuf;

use test_harness::test;
use test_harness::GhciWatchBuilder;
use tokio::process::Command;

/// Invalid experimental feature values should produce an error immediately.
#[test]
async fn invalid_experimental_feature_errors() {
    let result = GhciWatchBuilder::new("tests/data/simple")
        .with_args(["--experimental-features", "asdflkj"])
        .start()
        .await;
    assert!(
        result.is_err(),
        "ghciwatch should error on invalid experimental feature"
    );
}

/// Runs ghciwatch directly (not via `GhciWatchBuilder`) because TUI mode
/// crashes without a terminal, exiting before the test harness can connect.
#[tokio::test]
async fn experimental_features_emits_warning() {
    let log_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("experimental-warning");
    std::fs::create_dir_all(&log_dir).expect("can create log dir");
    let log_path = log_dir.join("ghciwatch.json");

    let _output = Command::new(env!("CARGO_BIN_EXE_ghciwatch"))
        .args(["--experimental-features", "tui"])
        .arg("--log-json")
        .arg(&log_path)
        .args(["--watch", "src"])
        .current_dir("tests/data/simple")
        .output()
        .await
        .expect("can run ghciwatch");

    let log_contents = std::fs::read_to_string(&log_path)
        .unwrap_or_else(|e| panic!("can read log file at {}: {e}", log_path.display()));
    assert!(
        log_contents.contains("--experimental-features"),
        "warning about experimental features should appear in JSON log"
    );

    let _ = std::fs::remove_dir_all(&log_dir);
}
