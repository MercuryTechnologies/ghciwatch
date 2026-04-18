use nix::sys::signal;
use nix::sys::signal::Signal;
use nix::unistd::Pid;
use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::Fs;
use test_harness::GhciWatch;
use test_harness::GhciWatchBuilder;
use test_harness::JsonValue;

/// Test that `ghciwatch` can gracefully shutdown on Ctrl-C.
#[test]
async fn can_shutdown_gracefully() {
    let mut session = GhciWatch::new("tests/data/simple")
        .await
        .expect("ghciwatch starts");
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    signal::kill(Pid::from_raw(session.pid() as i32), Signal::SIGINT)
        .expect("Failed to send Ctrl-C to ghciwatch");

    session
        .wait_for_log("^All tasks completed successfully$")
        .await
        .unwrap();

    let status = session.wait_until_exit().await.unwrap();
    assert!(status.success(), "ghciwatch exits successfully");
}

fn extract_pid(event: &test_harness::Event) -> i32 {
    match event.fields.get("pid").unwrap() {
        JsonValue::Number(pid) => pid,
        value => panic!("pid field has wrong type: {value:?}"),
    }
    .as_i64()
    .expect("pid is i64")
    .try_into()
    .expect("pid is i32")
}

/// Test that when the `ghci` process is unexpectedly killed, `ghciwatch` waits for a file change
/// and then restarts the session rather than shutting down.
#[test]
async fn restarts_after_ghci_killed() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .start()
        .await
        .expect("ghciwatch starts");

    let event = session
        .wait_for_startup_log(BaseMatcher::message("^Started ghci$"))
        .await
        .expect("ghciwatch starts ghci");
    let pid = extract_pid(&event);

    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    signal::kill(Pid::from_raw(pid), Signal::SIGKILL).expect("Failed to kill ghci");

    // ghciwatch should detect the exit and wait for a file change.
    session
        .wait_for_log("ghci exited unexpectedly")
        .await
        .expect("ghciwatch detects unexpected ghci exit");

    // A file change triggers the restart.
    session.clear_events();
    session
        .fs()
        .touch(session.path("src/MyLib.hs"))
        .await
        .expect("can touch source file");

    // ghciwatch should restart ghci and finish loading. After an unexpected exit, ghciwatch logs
    // "Finished restarting in X.Xs" (not "Finished starting up"), so we match on the common
    // suffix rather than ghci_started() which only matches "starting up".
    session
        .wait_for_startup_log(BaseMatcher::ghci_started())
        .await
        .expect("ghciwatch restarts ghci after unexpected exit");
}

/// Test that when ghci is killed, irrelevant file changes (non-Haskell, non-glob-matched) do not
/// trigger a restart, but a relevant Haskell file change does.
#[test]
async fn does_not_restart_on_irrelevant_file_change() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .start()
        .await
        .expect("ghciwatch starts");

    let event = session
        .wait_for_startup_log(BaseMatcher::message("^Started ghci$"))
        .await
        .expect("ghciwatch starts ghci");
    let pid = extract_pid(&event);

    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    signal::kill(Pid::from_raw(pid), Signal::SIGKILL).expect("Failed to kill ghci");

    session
        .wait_for_log("ghci exited unexpectedly")
        .await
        .expect("ghciwatch detects unexpected ghci exit");

    // Touch an irrelevant file inside the watched `src/` directory. The watcher will send the
    // event, but the classifier should skip it because it's not a Haskell source file and doesn't
    // match any reload/restart globs.
    session
        .fs()
        .touch(session.path("src/irrelevant.txt"))
        .await
        .expect("can touch irrelevant file");

    session
        .wait_for_log("File change not relevant to ghci; continuing to wait")
        .await
        .expect("ghciwatch skips irrelevant file change");

    // Now touch a relevant Haskell file to trigger the actual restart.
    session.clear_events();
    session
        .fs()
        .touch(session.path("src/MyLib.hs"))
        .await
        .expect("can touch source file");

    session
        .wait_for_startup_log(BaseMatcher::ghci_started())
        .await
        .expect("ghciwatch restarts ghci after relevant file change");
}

/// Test that when ghci fails to start repeatedly (e.g. a dependency won't compile), ghciwatch
/// keeps waiting for file changes and retrying rather than crashing after the first attempt.
#[test]
async fn handles_repeated_startup_failures() {
    let mut session = GhciWatchBuilder::new("tests/data/with-dep")
        .before_start(move |path| {
            // A version of SimpleDep.hs with an unclosed string literal — cabal will refuse to build it.
            async move {
                Fs::new()
                    .replace(
                        path.join("simple-dep/src/SimpleDep.hs"),
                        "\"depFunc\"",
                        "\"depFunc",
                    )
                    .await
            }
        })
        .start()
        .await
        .expect("ghciwatch starts");

    // First startup fails because simple-dep won't compile.
    session
        .wait_for_startup_log("ghci exited during startup")
        .await
        .expect("ghciwatch detects first startup failure");

    // Touching a source file triggers the first restart attempt, which also fails.
    session
        .fs()
        .touch(session.path("src/MyLib.hs"))
        .await
        .expect("can touch source file");

    // Clear events so we don't match the first "ghci exited during startup" again.
    session.clear_events();

    // The second failure confirms the retry loop re-enters rather than crashing.
    session
        .wait_for_startup_log("ghci exited during startup")
        .await
        .expect("ghciwatch detects second startup failure");

    // Fix the dependency, then trigger another restart.
    session
        .fs()
        .replace(
            session.path("simple-dep/src/SimpleDep.hs"),
            "\"depFunc",
            "\"depFunc\"",
        )
        .await
        .expect("can fix simple-dep");
    session
        .fs()
        .touch(session.path("src/MyLib.hs"))
        .await
        .expect("can touch source file");

    // This restart should succeed.
    session
        .wait_for_startup_log(BaseMatcher::message(
            r"Finished starting up in \d+\.\d+m?s$",
        ))
        .await
        .expect("ghciwatch restarts ghci after dependency is fixed");
}

/// Check that startup failures are handled correctly with `--before-restart-ghci` hooks.
#[test]
async fn handles_repeated_startup_failures_before_restart_ghci_hook() {
    let mut session = GhciWatchBuilder::new("tests/data/with-dep")
        .before_start(move |path| {
            // A version of SimpleDep.hs with an unclosed string literal — cabal will refuse to build it.
            async move {
                Fs::new()
                    .replace(
                        path.join("simple-dep/src/SimpleDep.hs"),
                        "\"depFunc\"",
                        "\"depFunc",
                    )
                    .await
            }
        })
        .with_args(["--before-restart-ghci", "putStrLn \"hello\""])
        .start()
        .await
        .expect("ghciwatch starts");

    // First startup fails because simple-dep won't compile.
    session
        .wait_for_startup_log("ghci exited during startup")
        .await
        .expect("ghciwatch detects first startup failure");

    // Touching a source file triggers the first restart attempt, which also fails.
    session
        .fs()
        .touch(session.path("src/MyLib.hs"))
        .await
        .expect("can touch source file");

    // Clear events so we don't match the first "ghci exited during startup" again.
    session.clear_events();

    // The second failure confirms the retry loop re-enters rather than crashing.
    session
        .wait_for_startup_log("ghci exited during startup")
        .await
        .expect("ghciwatch detects second startup failure");
}

/// Test that when ghci exits unexpectedly during a dispatched reload/restart (not during startup),
/// ghciwatch detects the exit and restarts on the next relevant file change.
///
/// This catches a bug where the dispatch `tokio::select!` did not poll `exited_receiver`, causing
/// the dispatch task to hold the ghci Mutex forever and deadlocking the retry-restart loop.
#[test]
async fn handles_unexpected_exit_during_dispatch() {
    let mut session = GhciWatchBuilder::new("tests/data/with-dep")
        .with_args([
            "--watch",
            "simple-dep",
            "--restart-glob",
            "simple-dep/src/*.hs",
        ])
        .start()
        .await
        .expect("ghciwatch starts");

    // Wait for successful initial startup.
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    // Introduce a syntax error in the dependency. Since it matches --restart-glob,
    // this triggers a restart. The restart fails because cabal can't build the broken
    // dependency, causing ghci to exit unexpectedly.
    session
        .fs()
        .replace(
            session.path("simple-dep/src/SimpleDep.hs"),
            "\"depFunc\"",
            "\"depFunc",
        )
        .await
        .expect("can break simple-dep");

    // ghciwatch should detect the unexpected exit (not "during startup").
    session
        .wait_for_log("ghci exited unexpectedly")
        .await
        .expect("ghciwatch detects unexpected exit during dispatch");

    // Fix the syntax error. This also matches --restart-glob, so it triggers a restart attempt.
    session
        .fs()
        .replace(
            session.path("simple-dep/src/SimpleDep.hs"),
            "\"depFunc",
            "\"depFunc\"",
        )
        .await
        .expect("can fix simple-dep");

    // ghciwatch should restart successfully.
    session
        .wait_for_startup_log(BaseMatcher::ghci_started())
        .await
        .expect("ghciwatch restarts ghci after fixing the dependency");
}
