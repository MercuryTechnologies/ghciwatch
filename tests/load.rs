use indoc::indoc;

use test_harness::fs;
use test_harness::test;
use test_harness::GhciWatch;

/// Test that `ghciwatch` can start up `ghci` and load a session.
#[test]
async fn can_load() {
    let mut session = GhciWatch::new("tests/data/simple")
        .await
        .expect("ghciwatch starts");
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");
}

/// Test that `ghciwatch` can load new modules.
#[test]
async fn can_load_new_module() {
    let mut session = GhciWatch::new("tests/data/simple")
        .await
        .expect("ghciwatch starts");
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");
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
        .expect("ghciwatch loads new modules");
}
