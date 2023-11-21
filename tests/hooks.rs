use std::time::Duration;

use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::GhciWatchBuilder;

/// Test that `ghciwatch` can run its lifecycle hooks.
///
/// The strategy here is to set a bunch of hooks that print simple messages. We use multiple hooks
/// just to test that it's allowed. Then we trigger the events that make the hooks run and confirm
/// that the hooks run.
#[test]
async fn can_run_hooks() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_args([
            "--before-startup-shell",
            "async:touch before-startup-shell-1",
            "--before-startup-shell",
            "touch before-startup-shell-2",
            // ---
            "--after-startup-ghci",
            "putStrLn \"after-startup-1\"",
            "--after-startup-ghci",
            "putStrLn \"after-startup-2\"",
            // ---
            "--before-reload-ghci",
            "putStrLn \"before-reload-1\"",
            "--before-reload-ghci",
            "putStrLn \"before-reload-2\"",
            // ---
            "--after-reload-ghci",
            "putStrLn \"after-reload-1\"",
            "--after-reload-ghci",
            "putStrLn \"after-reload-2\"",
            // ---
            "--after-reload-shell",
            "touch after-reload-shell-1",
            "--after-reload-shell",
            "async:touch after-reload-shell-2",
            // ---
            "--before-restart-ghci",
            "putStrLn \"before-restart-1\"",
            "--before-restart-ghci",
            "putStrLn \"before-restart-2\"",
            // ---
            "--after-restart-ghci",
            "putStrLn \"after-restart-1\"",
            "--after-restart-ghci",
            "putStrLn \"after-restart-2\"",
            // ---
            "--after-restart-shell",
            "async:touch after-restart-shell-1",
            "--after-restart-shell",
            "touch after-restart-shell-2",
        ])
        .start()
        .await
        .expect("ghciwatch starts");

    let wait_duration = Duration::from_secs(10);
    session
        .wait_for_log(
            BaseMatcher::message("Running before-startup command")
                .with_field("command", "touch before-startup-shell-1"),
        )
        .await
        .unwrap();
    session
        .fs()
        .wait_for_path(wait_duration, &session.path("before-startup-shell-1"))
        .await
        .unwrap();

    session
        .wait_for_log(
            BaseMatcher::message("Running before-startup command")
                .with_field("command", "touch before-startup-shell-2"),
        )
        .await
        .unwrap();
    session
        .fs()
        .wait_for_path(wait_duration, &session.path("before-startup-shell-2"))
        .await
        .unwrap();

    session
        .wait_for_log(
            BaseMatcher::message("Running after-startup command")
                .with_field("command", "putStrLn \"after-startup-1\""),
        )
        .await
        .unwrap();
    session
        .wait_for_log(BaseMatcher::message("Read line").with_field("line", "^after-startup-1$"))
        .await
        .unwrap();

    session
        .wait_for_log(
            BaseMatcher::message("Running after-startup command")
                .with_field("command", "putStrLn \"after-startup-2\""),
        )
        .await
        .unwrap();
    session
        .wait_for_log(BaseMatcher::message("Read line").with_field("line", "^after-startup-2$"))
        .await
        .unwrap();

    session.wait_until_ready().await.unwrap();

    session
        .fs()
        .touch(session.path("src/MyLib.hs"))
        .await
        .unwrap();

    // Before reload
    session
        .wait_for_log(
            BaseMatcher::message("Running before-reload command")
                .with_field("command", "putStrLn \"before-reload-1\""),
        )
        .await
        .unwrap();
    session
        .wait_for_log(BaseMatcher::message("Read line").with_field("line", "^before-reload-1$"))
        .await
        .unwrap();

    session
        .wait_for_log(
            BaseMatcher::message("Running before-reload command")
                .with_field("command", "putStrLn \"before-reload-2\""),
        )
        .await
        .unwrap();
    session
        .wait_for_log(BaseMatcher::message("Read line").with_field("line", "^before-reload-2$"))
        .await
        .unwrap();

    // After reload
    session
        .wait_for_log(
            BaseMatcher::message("Running after-reload command")
                .with_field("command", "putStrLn \"after-reload-1\""),
        )
        .await
        .unwrap();
    session
        .wait_for_log(BaseMatcher::message("Read line").with_field("line", "^after-reload-1$"))
        .await
        .unwrap();

    session
        .wait_for_log(
            BaseMatcher::message("Running after-reload command")
                .with_field("command", "putStrLn \"after-reload-2\""),
        )
        .await
        .unwrap();
    session
        .wait_for_log(BaseMatcher::message("Read line").with_field("line", "^after-reload-2$"))
        .await
        .unwrap();

    session
        .wait_for_log(
            BaseMatcher::message("Running after-reload command")
                .with_field("command", "touch after-reload-shell-1"),
        )
        .await
        .unwrap();
    session
        .fs()
        .wait_for_path(wait_duration, &session.path("before-startup-shell-1"))
        .await
        .unwrap();

    session
        .wait_for_log(
            BaseMatcher::message("Running after-reload command")
                .with_field("command", "touch after-reload-shell-2"),
        )
        .await
        .unwrap();
    session
        .fs()
        .wait_for_path(wait_duration, &session.path("before-startup-shell-2"))
        .await
        .unwrap();

    session
        .fs()
        .remove(session.path("src/MyModule.hs"))
        .await
        .unwrap();
    // Before restart
    session
        .wait_for_log(
            BaseMatcher::message("Running before-restart command")
                .with_field("command", "putStrLn \"before-restart-1\""),
        )
        .await
        .unwrap();
    session
        .wait_for_log(BaseMatcher::message("Read line").with_field("line", "^before-restart-1$"))
        .await
        .unwrap();

    session
        .wait_for_log(
            BaseMatcher::message("Running before-restart command")
                .with_field("command", "putStrLn \"before-restart-2\""),
        )
        .await
        .unwrap();
    session
        .wait_for_log(BaseMatcher::message("Read line").with_field("line", "^before-restart-2$"))
        .await
        .unwrap();

    // After restart
    session
        .wait_for_log(
            BaseMatcher::message("Running after-restart command")
                .with_field("command", "putStrLn \"after-restart-1\""),
        )
        .await
        .unwrap();
    session
        .wait_for_log(BaseMatcher::message("Read line").with_field("line", "^after-restart-1$"))
        .await
        .unwrap();

    session
        .wait_for_log(
            BaseMatcher::message("Running after-restart command")
                .with_field("command", "putStrLn \"after-restart-2\""),
        )
        .await
        .unwrap();
    session
        .wait_for_log(BaseMatcher::message("Read line").with_field("line", "^after-restart-2$"))
        .await
        .unwrap();

    session
        .wait_for_log(
            BaseMatcher::message("Running after-restart command")
                .with_field("command", "touch after-restart-shell-1"),
        )
        .await
        .unwrap();
    session
        .fs()
        .wait_for_path(wait_duration, &session.path("after-restart-shell-1"))
        .await
        .unwrap();

    session
        .wait_for_log(
            BaseMatcher::message("Running after-restart command")
                .with_field("command", "touch after-restart-shell-2"),
        )
        .await
        .unwrap();
    session
        .fs()
        .wait_for_path(wait_duration, &session.path("after-restart-shell-2"))
        .await
        .unwrap();
}
