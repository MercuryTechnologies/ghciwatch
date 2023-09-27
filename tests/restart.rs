use indoc::indoc;

use test_harness::fs;
use test_harness::test;
use test_harness::GhcidNg;
use test_harness::GhcidNgBuilder;
use test_harness::Matcher;

/// Test that `ghcid-ng` can restart `ghci` after a module is moved.
#[test]
async fn can_restart_after_module_move() {
    let mut session = GhcidNg::new("tests/data/simple")
        .await
        .expect("ghcid-ng starts");
    session
        .wait_until_ready()
        .await
        .expect("ghcid-ng loads ghci");

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
        .expect("ghcid-ng loads new modules");

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
        .expect("ghcid-ng restarts ghci");

    session
        .assert_logged(
            Matcher::message("Compiling")
                .in_span("reload")
                .with_field("module", r"My\.CoolModule"),
        )
        .await
        .unwrap();

    session
        .assert_logged(Matcher::message("Compilation succeeded").in_span("reload"))
        .await
        .unwrap();
}

/// Test that `ghcid-ng` can restart after a custom `--watch-restart` path changes.
#[test]
async fn can_restart_on_custom_file_change() {
    let mut session = GhcidNgBuilder::new("tests/data/simple")
        .with_args(["--watch-restart", "package.yaml"])
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
