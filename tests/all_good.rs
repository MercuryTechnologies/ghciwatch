use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::GhciWatchBuilder;
use test_harness::Matcher;

/// Test that `ghciwatch` can detect when compilation fails.
///
/// Regression test for DUX-1649.
#[test]
async fn can_detect_compilation_failure() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .start()
        .await
        .expect("ghciwatch starts");
    let module_path = session.path("src/MyModule.hs");

    session.wait_until_ready().await.expect("ghciwatch loads");

    session
        .fs()
        .replace(&module_path, "example :: String", "example :: ()")
        .await
        .unwrap();

    session
        .wait_for_log(BaseMatcher::compilation_failed())
        .await
        .unwrap();

    session
        .wait_for_log(BaseMatcher::reload_completes().but_not(BaseMatcher::message("All good!")))
        .await
        .unwrap();

    session
        .fs()
        .replace(&module_path, "example :: ()", "example :: String")
        .await
        .unwrap();

    session
        .wait_for_log(BaseMatcher::message("All good!"))
        .await
        .unwrap();
}

/// Test that `ghciwatch` can detect an `*** Exception` diagnostic.
///
/// Regression test for DUX-3144.
#[test]
async fn can_detect_exception() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .start()
        .await
        .expect("ghciwatch starts");
    session.wait_until_ready().await.expect("ghciwatch loads");

    let module_path = session.path("src/MyModule.hs");

    session
        .fs()
        .prepend(&module_path, "{-# OPTIONS_GHC -F -pgmF false #-}\n")
        .await
        .unwrap();

    session
        .wait_for_log(BaseMatcher::compilation_failed())
        .await
        .unwrap();

    session
        .wait_for_log(BaseMatcher::reload_completes().but_not(BaseMatcher::message("All good!")))
        .await
        .unwrap();
}
