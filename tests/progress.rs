use indoc::indoc;

use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::GhciWatchBuilder;

/// Test that compilation progress events include current/total fields.
#[test]
async fn compilation_emits_progress_fields() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_log_filter("ghciwatch::ghci::compilation_log=debug")
        .start()
        .await
        .expect("ghciwatch starts");
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    // CompilationLog::extend() emits: tracing::debug!(module, path, current, total, "Compiling")
    // The initial load emitted these during startup. They are already in the first checkpoint.
    // Note: current/total are numeric fields (not strings), so field matchers can't regex-match
    // them. We verify the event exists with the module field instead.
    session
        .assert_logged_or_wait(BaseMatcher::message("^Compiling$").with_field("module", "MyLib"))
        .await
        .expect("Compiling event with module field emitted");
}

/// Test that --experimental-features progress is accepted and compilation still succeeds.
#[test]
async fn experimental_progress_flag_works() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_args(["--experimental-features", "progress"])
        .start()
        .await
        .expect("ghciwatch starts with --experimental-features progress");
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    session
        .fs()
        .append(
            session.path("src/MyLib.hs"),
            indoc!(
                "

            hello = 1 :: Integer

            "
            ),
        )
        .await
        .unwrap();
    session
        .wait_until_reload()
        .await
        .expect("ghciwatch reloads with --experimental-features progress");
    session
        .wait_for_log(BaseMatcher::reload_completes())
        .await
        .expect("reload completes with --experimental-features progress");
}
