use indoc::indoc;

use test_harness::fs;
use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::GhciWatch;
use test_harness::GhciWatchBuilder;

/// Test that `ghciwatch` can restart `ghci` after a module is moved.
#[test]
async fn can_restart_after_module_move() {
    let mut session = GhciWatch::new("tests/data/simple")
        .await
        .expect("ghciwatch starts");
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    let module_path = session.path("src/My/Module.hs");
    fs::write(
        &module_path,
        indoc!(
            "module My.Module (myIdent) where
            myIdent :: ()
            myIdent = ()
            "
        ),
    )
    .await
    .unwrap();
    session
        .wait_until_add()
        .await
        .expect("ghciwatch loads new modules");

    {
        // Rename the module and fix the module name to match the new path.
        let contents = fs::read(&module_path).await.unwrap();
        fs::remove(&module_path).await.unwrap();
        fs::write(
            session.path("src/My/CoolModule.hs"),
            contents.replace("module My.Module", "module My.CoolModule"),
        )
        .await
        .unwrap();
    }

    session
        .wait_until_restart()
        .await
        .expect("ghciwatch restarts ghci");

    session
        .wait_for_log(
            BaseMatcher::message("Compiling")
                .in_span("reload")
                .with_field("module", r"My\.CoolModule"),
        )
        .await
        .unwrap();

    session
        .wait_for_log(BaseMatcher::message("Compilation succeeded").in_span("reload"))
        .await
        .unwrap();
}

/// Test that `ghciwatch` can restart after a custom `--watch-restart` path changes.
#[test]
async fn can_restart_on_custom_file_change() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_args(["--watch-restart", "package.yaml"])
        .start()
        .await
        .expect("ghciwatch starts");

    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    fs::touch(session.path("package.yaml")).await.unwrap();

    session
        .wait_until_restart()
        .await
        .expect("ghciwatch restarts when package.yaml changes");
}
