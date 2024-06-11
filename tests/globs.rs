use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::Fs;
use test_harness::GhciWatchBuilder;
use test_harness::Matcher;

/// Test that `ghciwatch` can reload when a file matching a `--reload-glob` is changed.
#[test]
async fn can_reload_glob() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_args(["--reload-glob", "**/*.persistentmodels"])
        .start()
        .await
        .expect("ghciwatch starts");
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    session
        .fs()
        .touch(session.path("src/my_model.persistentmodels"))
        .await
        .unwrap();

    session
        .wait_until_reload()
        .await
        .expect("ghciwatch reloads when a `.persistentmodels` file is created");
}

/// Test that `ghciwatch` skips reloading when a file matching an exclude `--reload-glob` is
/// changed.
#[test]
async fn can_skip_reload_for_ignore_glob() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_args(["--reload-glob", "!**/*.hs"])
        .start()
        .await
        .expect("ghciwatch starts");
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    session
        .fs()
        .touch(session.path("src/MyModule.hs"))
        .await
        .unwrap();

    session
        .wait_for_log(BaseMatcher::reload_completes().but_not(BaseMatcher::reload()))
        .await
        .expect("ghciwatch reloads when a `.persistentmodels` file is created");
}

/// Test that `ghciwatch` can restart when a file matching a `--restart-glob` is changed.
#[test]
async fn can_restart_on_custom_file_change() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_args(["--restart-glob", "package.yaml", "--watch", "package.yaml"])
        .start()
        .await
        .expect("ghciwatch starts");

    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    session
        .fs()
        .touch(session.path("package.yaml"))
        .await
        .unwrap();

    session
        .wait_until_restart()
        .await
        .expect("ghciwatch restarts when package.yaml changes");
}

/// Test that `ghciwatch` can restart when a `.cabal` file is changed.
#[test]
async fn can_restart_on_cabal_file_change() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .before_start(|project_root| async move {
            Fs::new()
                .touch(project_root.join("my-simple-package.cabal"))
                .await
        })
        .with_args(["--watch", "my-simple-package.cabal"])
        .start()
        .await
        .expect("ghciwatch starts");

    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    session
        .fs()
        .touch(session.path("my-simple-package.cabal"))
        .await
        .unwrap();

    session
        .wait_until_restart()
        .await
        .expect("ghciwatch restarts when .cabal files change");
}

/// Test that `ghciwatch` can restart when a `.ghci` file is changed.
#[test]
async fn can_restart_on_ghci_file_change() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .before_start(
            |project_root| async move { Fs::new().touch(project_root.join(".ghci")).await },
        )
        .with_args(["--watch", ".ghci"])
        .start()
        .await
        .expect("ghciwatch starts");

    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    session.fs().touch(session.path(".ghci")).await.unwrap();

    session
        .wait_until_restart()
        .await
        .expect("ghciwatch restarts when .ghci files change");
}

/// Test that `ghciwatch` doesn't restart when a `.ghci` file is changed when `--restart-glob !.ghci`
/// is given.
#[test]
async fn can_ignore_restart_paths() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .before_start(
            |project_root| async move { Fs::new().touch(project_root.join(".ghci")).await },
        )
        .with_args(["--restart-glob", "!.ghci", "--watch", ".ghci"])
        .start()
        .await
        .expect("ghciwatch starts");

    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    session.fs().touch(session.path(".ghci")).await.unwrap();

    session
        .wait_for_log(BaseMatcher::reload_completes().but_not(BaseMatcher::restart()))
        .await
        .expect("ghciwatch doesn't restart when ignored globs are changed");
}

/// Ghciwatch can ignore when a file is removed.
#[test]
async fn can_ignore_removal() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .before_start(|project_root| async move {
            Fs::new().touch(project_root.join("my-model.db")).await
        })
        .with_args(["--reload-glob", "!**/*.db", "--watch", "."])
        .start()
        .await
        .expect("ghciwatch starts");

    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    session
        .fs()
        .remove(session.path("my-model.db"))
        .await
        .unwrap();

    session
        .wait_for_log(BaseMatcher::reload_completes().but_not(BaseMatcher::ghci_remove()))
        .await
        .expect("Ghciwatch ignores removed files");
}

/// Ghciwatch can whitelist reloads for removed files.
#[test]
async fn can_whitelist_removal() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .before_start(|project_root| async move {
            Fs::new().touch(project_root.join("my-model.db")).await
        })
        .with_args(["--reload-glob", "**/*.db", "--watch", "."])
        .start()
        .await
        .expect("ghciwatch starts");

    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    session
        .fs()
        .remove(session.path("my-model.db"))
        .await
        .unwrap();

    session
        .wait_for_log(BaseMatcher::reload_completes().but_not(BaseMatcher::ghci_remove()))
        .await
        .expect("Ghciwatch ignores removed files");
}
