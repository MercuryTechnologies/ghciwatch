use test_harness::fs;
use test_harness::test;
use test_harness::GhciWatchBuilder;

/// Test that `ghciwatch` can start with compile errors.
///
/// This is a regression test for [#43](https://github.com/MercuryTechnologies/ghciwatch/issues/43).
#[test]
async fn can_start_with_failed_modules() {
    let module_path = "src/MyModule.hs";
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .before_start(move |path| async move {
            fs::replace(path.join(module_path), "example :: String", "example :: ()").await
        })
        .start()
        .await
        .expect("ghciwatch starts");
    let module_path = session.path(module_path);

    session
        .assert_logged("Compilation failed")
        .await
        .expect("ghciwatch fails to load with errors");

    session.wait_until_ready().await.expect("ghciwatch loads");

    fs::replace(&module_path, "example :: ()", "example :: String")
        .await
        .unwrap();

    session
        .assert_logged("Compilation succeeded")
        .await
        .expect("ghciwatch reloads fixed modules");
}
