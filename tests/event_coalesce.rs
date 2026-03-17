use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::GhciWatch;
use test_harness::GhciWatchBuilder;

/// Test that rapid file modifications are handled correctly, either by being
/// coalesced into a single reload or by being processed sequentially without
/// losing events.
#[test]
async fn rapid_file_changes_are_not_lost() {
    let mut session = GhciWatch::new("tests/data/simple")
        .await
        .expect("ghciwatch starts");
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    // Use `append` which has no load-bearing sleep, so both writes happen
    // back-to-back. The poll watcher should detect both changes in the same
    // cycle, causing them to be queued together on the channel and coalesced
    // by the greedy drain in `run_ghci`.
    session
        .fs()
        .append(
            session.path("src/MyLib.hs"),
            "\nhello = 1 :: Integer\n",
        )
        .await
        .unwrap();
    session
        .fs()
        .append(
            session.path("src/MyModule.hs"),
            "\nworld = 2 :: Integer\n",
        )
        .await
        .unwrap();

    // Both files' changes should be processed (either as a single coalesced
    // reload or as sequential reloads) and compilation should succeed.
    session
        .wait_for_log(BaseMatcher::compilation_succeeded())
        .await
        .expect("compilation succeeds after rapid writes");
}

/// Test that events are not silently lost when `--no-interrupt-reloads` is set
/// and a new file change arrives during an in-progress reload.
///
/// Before the fix, the new event was consumed from the channel but never acted
/// upon, causing the file change to be silently dropped.
#[test]
async fn no_interrupt_reloads_preserves_events() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_arg("--no-interrupt-reloads")
        .start()
        .await
        .expect("ghciwatch starts");
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    // Modify first file to trigger a reload.
    session
        .fs()
        .append(
            session.path("src/MyLib.hs"),
            "\nhello = 1 :: Integer\n",
        )
        .await
        .unwrap();

    // Wait for the reload to begin.
    session
        .wait_until_reload()
        .await
        .expect("first reload starts");

    // Modify second file. This may arrive while the first reload is still in
    // progress. With `--no-interrupt-reloads`, the reload won't be interrupted,
    // but the event must be preserved for the next iteration.
    session
        .fs()
        .append(
            session.path("src/MyModule.hs"),
            "\nworld = 2 :: Integer\n",
        )
        .await
        .unwrap();

    // Wait for the first reload to finish.
    session
        .wait_for_log(BaseMatcher::reload_completes())
        .await
        .expect("first reload completes");

    // The second file's changes must eventually trigger another reload.
    // Before the fix, this event was silently dropped and would time out here.
    session
        .wait_until_reload()
        .await
        .expect("second reload starts for queued event");
    session
        .wait_for_log(BaseMatcher::reload_completes())
        .await
        .expect("second reload completes");
}
