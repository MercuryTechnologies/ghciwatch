use std::time::Duration;

use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::GhciWatchBuilder;
// These tests use `tests/data/slow-compile`, which contains a TemplateHaskell
// module (`SlowModule`) that sleeps for 500ms at compile time. This makes
// reloads that touch SlowModule slow enough for file-change events to arrive
// mid-reload, exercising the event batching code paths.

/// Test that edits made during an in-progress compile are batched into a single
/// follow-up compile, rather than triggering one compile per edit.
///
/// Scenario: a compile is running, we make 3 edits before it finishes, and
/// expect exactly one more compile afterward containing all 3 edits.
///
/// Uses `--no-interrupt-reloads` so the in-progress compile runs to completion
/// while events queue up, exercising the event preservation in the `else`
/// branch and the greedy drain that merges pending events.
#[test]
async fn edits_during_compile_are_batched() {
    let mut session = GhciWatchBuilder::new("tests/data/slow-compile")
        .with_arg("--no-interrupt-reloads")
        .with_poll_interval("100ms")
        .with_default_timeout(Duration::from_secs(30))
        .with_startup_timeout(Duration::from_secs(120))
        .start()
        .await
        .expect("ghciwatch starts");
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    // Modify SlowModule.hs to trigger a slow reload. GHCi only re-runs TH
    // splices for modules whose source files actually changed, so we must
    // modify SlowModule.hs itself (not a dependency).
    session
        .fs()
        .append(
            session.path("src/SlowModule.hs"),
            "\nslowExtra = 99 :: Int\n",
        )
        .await
        .unwrap();

    // Wait for the slow reload to begin.
    session
        .wait_until_reload()
        .await
        .expect("first reload starts");

    // While the reload is running, make 3 edits using `append` (no
    // load-bearing sleep). All 3 writes land almost simultaneously, so the
    // watcher detects them in the same poll cycle and they arrive on the
    // channel while the slow reload is still in progress.
    session
        .fs()
        .append(session.path("src/ExtraA.hs"), "\nextraA2 = 1 :: Integer\n")
        .await
        .unwrap();
    session
        .fs()
        .append(session.path("src/ExtraB.hs"), "\nextraB2 = 2 :: Integer\n")
        .await
        .unwrap();
    session
        .fs()
        .append(session.path("src/ExtraC.hs"), "\nextraC2 = 3 :: Integer\n")
        .await
        .unwrap();

    // Wait for the first (slow) reload to finish.
    session
        .wait_for_log(BaseMatcher::reload_completes())
        .await
        .expect("first reload completes");

    // The follow-up reload should contain ALL 3 edited files in a single
    // reload (merged). Without the fix, events are silently dropped and each
    // file triggers its own separate reload.
    session
        .wait_for_log(BaseMatcher::message(
            "(?s)Reloading ghci:.*ExtraA.*ExtraB.*ExtraC",
        ))
        .await
        .expect("second reload includes all 3 edited files in one reload");

    session
        .wait_for_log(BaseMatcher::compilation_succeeded())
        .await
        .expect("second reload compiles successfully");
}
