use indoc::indoc;

use test_harness::fs;
use test_harness::test;
use test_harness::GhcidNg;
use test_harness::Matcher;

/// Test that `ghcid-ng` can start up `ghci` and load a session.
#[test]
async fn can_load() {
    let mut session = GhcidNg::new("tests/data/simple")
        .await
        .expect("ghcid-ng starts");
    session
        .wait_until_ready()
        .await
        .expect("ghcid-ng loads ghci");
}

/// Test that `ghcid-ng` can start up and then reload on changes.
#[test]
async fn can_reload() {
    let mut session = GhcidNg::new("tests/data/simple")
        .await
        .expect("ghcid-ng starts");
    session
        .wait_until_ready()
        .await
        .expect("ghcid-ng loads ghci");
    fs::append(
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
        .expect("ghcid-ng reloads on changes");
    session
        .assert_logged(
            Matcher::span_close()
                .in_module("ghcid_ng::ghci")
                .in_spans(["on_action", "reload"]),
        )
        .await
        .expect("ghcid-ng finishes reloading");
}

/// Test that `ghcid-ng` can load new modules.
#[test]
async fn can_load_new_module() {
    let mut session = GhcidNg::new("tests/data/simple")
        .await
        .expect("ghcid-ng starts");
    session
        .wait_until_ready()
        .await
        .expect("ghcid-ng loads ghci");
    fs::write(
        session.path("src/My/Module.hs"),
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
}

/// Test that `ghcid-ng` can reload a module that fails to compile.
#[test]
async fn can_reload_after_error() {
    let mut session = GhcidNg::new("tests/data/simple")
        .await
        .expect("ghcid-ng starts");
    session
        .wait_until_ready()
        .await
        .expect("ghcid-ng loads ghci");
    let new_module = session.path("src/My/Module.hs");

    fs::write(
        &new_module,
        indoc!(
            "module My.Module (myIdent) where
            myIdent :: ()
            myIdent = \"Uh oh!\"
            "
        ),
    )
    .await
    .unwrap();
    session
        .wait_until_add()
        .await
        .expect("ghcid-ng loads new modules");
    session
        .assert_logged(Matcher::message("Compilation failed").in_spans(["reload", "add_module"]))
        .await
        .unwrap();

    fs::replace(&new_module, "myIdent = \"Uh oh!\"", "myIdent = ()")
        .await
        .unwrap();

    session
        .wait_until_add()
        .await
        .expect("ghcid-ng reloads on changes");
    session
        .assert_logged(Matcher::message("Compilation succeeded").in_span("reload"))
        .await
        .unwrap();
}

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
