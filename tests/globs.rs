use test_harness::fs;
use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::GhciWatchBuilder;
use test_harness::Matcher;

/// Test that `ghcid-ng` can reload when a file matching a `--reload-glob` is changed.
#[test]
async fn can_reload_glob() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_args(["--reload-glob", "**/*.persistentmodels"])
        .start()
        .await
        .expect("ghcid-ng starts");
    session
        .wait_until_ready()
        .await
        .expect("ghcid-ng loads ghci");

    fs::touch(session.path("src/my_model.persistentmodels"))
        .await
        .unwrap();

    session
        .wait_until_reload()
        .await
        .expect("ghcid-ng reloads when a `.persistentmodels` file is created");
}

/// Test that `ghcid-ng` skips reloading when a file matching an exclude `--reload-glob` is
/// changed.
#[test]
async fn can_skip_reload_for_ignore_glob() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_args(["--reload-glob", "!**/*.hs"])
        .start()
        .await
        .expect("ghcid-ng starts");
    session
        .wait_until_ready()
        .await
        .expect("ghcid-ng loads ghci");

    fs::touch(session.path("src/MyModule.hs")).await.unwrap();

    session
        .wait_for_log(BaseMatcher::reload_completes().but_not(BaseMatcher::reload()))
        .await
        .expect("ghcid-ng reloads when a `.persistentmodels` file is created");
}

/// Test that `ghcid-ng` can restart when a file matching a `--restart-glob` is changed.
#[test]
async fn can_restart_on_custom_file_change() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_args(["--restart-glob", "package.yaml", "--watch", "package.yaml"])
        .start()
        .await
        .expect("ghcid-ng starts");

    session
        .wait_until_ready()
        .await
        .expect("ghcid-ng loads ghci");

    fs::touch(session.path("package.yaml")).await.unwrap();

    session
        .wait_until_restart()
        .await
        .expect("ghcid-ng restarts when package.yaml changes");
}

/// Test that `ghcid-ng` can restart when a `.cabal` file is changed.
#[test]
async fn can_restart_on_cabal_file_change() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .before_start(|project_root| async move {
            fs::touch(project_root.join("my-simple-package.cabal")).await
        })
        .with_args(["--watch", "my-simple-package.cabal"])
        .start()
        .await
        .expect("ghcid-ng starts");

    session
        .wait_until_ready()
        .await
        .expect("ghcid-ng loads ghci");

    fs::touch(session.path("my-simple-package.cabal"))
        .await
        .unwrap();

    session
        .wait_until_restart()
        .await
        .expect("ghcid-ng restarts when .cabal files change");
}

/// Test that `ghcid-ng` can restart when a `.ghci` file is changed.
#[test]
async fn can_restart_on_ghci_file_change() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .before_start(|project_root| async move { fs::touch(project_root.join(".ghci")).await })
        .with_args(["--watch", ".ghci"])
        .start()
        .await
        .expect("ghcid-ng starts");

    session
        .wait_until_ready()
        .await
        .expect("ghcid-ng loads ghci");

    fs::touch(session.path(".ghci")).await.unwrap();

    session
        .wait_until_restart()
        .await
        .expect("ghcid-ng restarts when .ghci files change");
}

/// Test that `ghcid-ng` doesn't restart when a `.ghci` file is changed when `--restart-glob !.ghci`
/// is given.
#[test]
async fn can_ignore_restart_paths() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .before_start(|project_root| async move { fs::touch(project_root.join(".ghci")).await })
        .with_args(["--restart-glob", "!.ghci", "--watch", ".ghci"])
        .start()
        .await
        .expect("ghcid-ng starts");

    session
        .wait_until_ready()
        .await
        .expect("ghcid-ng loads ghci");

    fs::touch(session.path(".ghci")).await.unwrap();

    session
        .wait_for_log(BaseMatcher::reload_completes().but_not(BaseMatcher::restart()))
        .await
        .expect("ghcid-ng doesn't restart when ignored globs are changed");
}

/// Test that `ghcid-ng` restarts when a Haskell module is removed, even if a `--restart-glob`
/// explicitly ignores the path.
///
/// This is needed to work around a `ghci` bug: https://gitlab.haskell.org/ghc/ghc/-/issues/11596
#[test]
async fn can_restart_on_module_change_even_if_ignored() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_args(["--restart-glob", "!**/*.hs"])
        .start()
        .await
        .expect("ghcid-ng starts");

    session
        .wait_until_ready()
        .await
        .expect("ghcid-ng loads ghci");

    fs::remove(session.path("src/MyModule.hs")).await.unwrap();

    session
        .wait_until_restart()
        .await
        .expect("ghcid-ng restarts when Haskell files are removed");
}
