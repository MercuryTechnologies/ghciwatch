use indoc::indoc;

use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::GhciWatchBuilder;

/// Test that `ghciwatch` clears the screen on reloads and restarts when `--clear` is used.
#[test]
async fn clears_on_reload_and_restart() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_arg("--clear")
        .with_tracing_filter("ghciwatch::ghci[clear]=trace")
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

    {
        // Rename the module and fix the module name to match the new path.
        let module_path = session.path("src/MyModule.hs");
        let new_path = session.path("src/MyCoolModule.hs");
        session.fs_mut().disable_load_bearing_sleep();
        session.fs().rename(&module_path, &new_path).await.unwrap();
        session
            .fs()
            .replace(&new_path, "module MyModule", "module MyCoolModule")
            .await
            .unwrap();
        session.fs_mut().reset_load_bearing_sleep();
    }

    session.wait_for_log("Clearing the screen").await.unwrap();
    session
        .wait_until_restart()
        .await
        .expect("ghciwatch restarts ghci");
}
