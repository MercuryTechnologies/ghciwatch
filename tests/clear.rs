use indoc::indoc;

use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::GhciWatchBuilder;

/// Test that `ghciwatch` clears the screen on reloads and restarts when `--clear` is used.
#[test]
async fn clears_on_reload_and_restart() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_arg("--clear")
        .with_log_filter("ghciwatch::ghci[clear]=trace")
        .start()
        .await
        .expect("ghciwatch starts");
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

    session.wait_for_log("Clearing the screen").await.unwrap();
    session
        .wait_until_reload()
        .await
        .expect("ghciwatch reloads on changes");
    session
        .wait_for_log(BaseMatcher::reload_completes())
        .await
        .unwrap();

    // Modify the `package.yaml` to trigger a restart.
    session
        .fs()
        .append(session.path("package.yaml"), "\n")
        .await
        .unwrap();

    session.wait_for_log("Clearing the screen").await.unwrap();
    session
        .wait_until_restart()
        .await
        .expect("ghciwatch restarts ghci");
}
