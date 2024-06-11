use std::time::Duration;

use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::Fs;
use test_harness::GhciWatch;
use test_harness::GhciWatchBuilder;
use test_harness::SpanMatcher;

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

    // Modify the `package.yaml` to trigger a restart.
    session
        .fs()
        .append(session.path("package.yaml"), "\n")
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

/// Test that `ghciwatch` lifecycle hooks can observe the error log.
///
/// That is, the error log is updated before `--after-startup-shell` and `--after-reload-shell`.
#[test]
async fn hooks_can_observe_error_log() {
    let module_path = "src/MyModule.hs";
    let after_startup = shell_requote("grep -q '^src/MyModule.hs:4:11' ghcid.txt");
    let after_reload = shell_requote("grep -q '^src/MyModule.hs:5:11' ghcid.txt");
    let after_restart = shell_requote("grep -q '^src/MyCoolModule.hs:1:8' ghcid.txt");

    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .before_start(move |path| async move {
            Fs::new()
                .replace(path.join(module_path), "example :: String", "example :: ()")
                .await
        })
        .with_args([
            "--errors",
            "ghcid.txt",
            "--after-startup-shell",
            &after_startup,
            "--after-reload-shell",
            &after_reload,
            "--after-restart-shell",
            &after_restart,
        ])
        .with_log_filter("ghciwatch::ghci[run_hooks]=trace")
        .start()
        .await
        .expect("ghciwatch starts");

    session
        .wait_for_log(
            BaseMatcher::message("Running after-startup command")
                .with_field("command", &regex::escape(&after_startup)),
        )
        .await
        .unwrap();
    session
        .wait_for_log(
            BaseMatcher::message("grep finished successfully")
                .in_spans([SpanMatcher::new("run_hooks").with_field("event", "after-startup")]),
        )
        .await
        .unwrap();

    session.wait_until_ready().await.unwrap();

    let module_path = session.path(module_path);

    // Add a newline to change the line numbers. This makes each hook unique.
    session
        .fs()
        .replace(&module_path, "example :: ()", "\nexample :: ()")
        .await
        .unwrap();

    session
        .wait_for_log(
            BaseMatcher::message("Running after-reload command")
                .with_field("command", &regex::escape(&after_reload)),
        )
        .await
        .unwrap();
    session
        .wait_for_log(
            BaseMatcher::message("grep finished successfully")
                .in_spans([SpanMatcher::new("run_hooks").with_field("event", "after-reload")]),
        )
        .await
        .unwrap();

    {
        session.fs_mut().disable_load_bearing_sleep();
        // Rename the module.
        // This generates an error message we can observe in the error log, but it doesn't restart
        // the GHCi session so we need to touch the `.cabal` file for that...
        let new_path = session.path("src/MyCoolModule.hs");
        session.fs().rename(module_path, new_path).await.unwrap();

        // Modify the `package.yaml` to trigger a restart.
        session
            .fs()
            .append(session.path("package.yaml"), "\n")
            .await
            .unwrap();

        session.fs_mut().reset_load_bearing_sleep();
    }

    session
        .wait_for_log(
            BaseMatcher::message("Running after-restart command")
                .with_field("command", &regex::escape(&after_restart)),
        )
        .await
        .unwrap();
    session
        .wait_for_log(
            BaseMatcher::message("grep finished successfully")
                .in_spans([SpanMatcher::new("run_hooks").with_field("event", "after-restart")]),
        )
        .await
        .unwrap();
}

fn shell_requote(cmd: &str) -> String {
    shell_words::join(shell_words::split(cmd).unwrap())
}
