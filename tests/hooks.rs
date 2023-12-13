use std::time::Duration;

use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::GhciWatch;
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
            "async:touch before-startup-1",
            "--before-startup-shell",
            "touch before-startup-2",
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
            "--before-reload-shell",
            "touch before-reload-1",
            "--before-reload-shell",
            "async:touch before-reload-2",
            // ---
            "--after-reload-ghci",
            "putStrLn \"after-reload-1\"",
            "--after-reload-ghci",
            "putStrLn \"after-reload-2\"",
            // ---
            "--after-reload-shell",
            "touch after-reload-1",
            "--after-reload-shell",
            "async:touch after-reload-2",
            // ---
            "--before-restart-ghci",
            "putStrLn \"before-restart-1\"",
            "--before-restart-ghci",
            "putStrLn \"before-restart-2\"",
            // ---
            "--before-restart-shell",
            "touch before-restart-1",
            "--before-restart-shell",
            "async:touch before-restart-2",
            // ---
            "--after-restart-ghci",
            "putStrLn \"after-restart-1\"",
            "--after-restart-ghci",
            "putStrLn \"after-restart-2\"",
            // ---
            "--after-restart-shell",
            "async:touch after-restart-1",
            "--after-restart-shell",
            "touch after-restart-2",
        ])
        .start()
        .await
        .expect("ghciwatch starts");

    shell_hook(&mut session, "before-startup", "1").await;
    shell_hook(&mut session, "before-startup", "2").await;

    ghci_hook(&mut session, "after-startup", "1").await;
    ghci_hook(&mut session, "after-startup", "2").await;

    session.wait_until_ready().await.unwrap();

    session
        .fs()
        .touch(session.path("src/MyLib.hs"))
        .await
        .unwrap();

    shell_hook(&mut session, "before-reload", "1").await;
    shell_hook(&mut session, "before-reload", "2").await;

    ghci_hook(&mut session, "before-reload", "1").await;
    ghci_hook(&mut session, "before-reload", "2").await;

    shell_hook(&mut session, "after-reload", "1").await;
    shell_hook(&mut session, "after-reload", "2").await;

    ghci_hook(&mut session, "after-reload", "1").await;
    ghci_hook(&mut session, "after-reload", "2").await;

    session
        .fs()
        .remove(session.path("src/MyModule.hs"))
        .await
        .unwrap();

    shell_hook(&mut session, "before-restart", "1").await;
    shell_hook(&mut session, "before-restart", "2").await;

    ghci_hook(&mut session, "before-restart", "1").await;
    ghci_hook(&mut session, "before-restart", "2").await;

    shell_hook(&mut session, "after-restart", "1").await;
    shell_hook(&mut session, "after-restart", "2").await;

    ghci_hook(&mut session, "after-restart", "1").await;
    ghci_hook(&mut session, "after-restart", "2").await;
}

async fn ghci_hook(session: &mut GhciWatch, hook: &str, index: &str) {
    session
        .wait_for_log(
            BaseMatcher::message(&format!("Running {hook} command"))
                .with_field("command", &format!("putStrLn \"{hook}-{index}\"")),
        )
        .await
        .unwrap();
    session
        .wait_for_log(
            BaseMatcher::message("Read line").with_field("line", &format!("^{hook}-{index}$")),
        )
        .await
        .unwrap();
}

async fn shell_hook(session: &mut GhciWatch, hook: &str, index: &str) {
    let wait_duration = Duration::from_secs(10);
    session
        .wait_for_log(
            BaseMatcher::message(&format!("Running {hook} command"))
                .with_field("command", &format!("touch {hook}-{index}")),
        )
        .await
        .unwrap();
    session
        .fs()
        .wait_for_path(wait_duration, &session.path(format!("{hook}-{index}")))
        .await
        .unwrap();
}
