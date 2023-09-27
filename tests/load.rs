use indoc::indoc;

use test_harness::fs;
use test_harness::test;
use test_harness::GhcidNg;

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
