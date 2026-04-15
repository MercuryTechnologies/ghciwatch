use test_harness::test;
use test_harness::GhciWatchBuilder;

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
#[test]
async fn experimental_features_emits_warning() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_args(["--experimental-features", "progress"])
        .start()
        .await
        .expect("ghciwatch starts");

    session
        .wait_for_log(
            "`--experimental-features` may contain bugs or change drastically in future releases.",
        )
        .await
        .unwrap();
}
