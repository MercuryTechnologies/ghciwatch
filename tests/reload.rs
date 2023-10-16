use indoc::indoc;

use test_harness::fs;
use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::GhciWatch;

/// Test that `ghciwatch` can start up and then reload on changes.
#[test]
async fn can_reload() {
    let mut session = GhciWatch::new("tests/data/simple")
        .await
        .expect("ghciwatch starts");
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");
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
        .expect("ghciwatch reloads on changes");
    session
        .wait_for_log(
            BaseMatcher::span_close()
                .in_module("ghciwatch::ghci")
                .in_spans(["on_action", "reload"]),
        )
        .await
        .expect("ghciwatch finishes reloading");
}

/// Test that `ghciwatch` can reload a module that fails to compile.
#[test]
async fn can_reload_after_error() {
    let mut session = GhciWatch::new("tests/data/simple")
        .await
        .expect("ghciwatch starts");
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");
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
        .expect("ghciwatch loads new modules");
    session
        .wait_for_log(BaseMatcher::message("Compilation failed").in_spans(["reload", "add_module"]))
        .await
        .unwrap();

    fs::replace(&new_module, "myIdent = \"Uh oh!\"", "myIdent = ()")
        .await
        .unwrap();

    session
        .wait_until_add()
        .await
        .expect("ghciwatch reloads on changes");
    session
        .wait_for_log(BaseMatcher::message("Compilation succeeded").in_span("reload"))
        .await
        .unwrap();
}
