use expect_test::expect;
use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::GhciWatchBuilder;

/// Test that `ghciwatch --test ...` can run a test suite.
#[test]
async fn can_run_test_suite_on_reload() {
    let error_path = "ghcid.txt";
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_args(["--test-ghci", "TestMain.testMain", "--errors", error_path])
        .start()
        .await
        .expect("ghciwatch starts");
    let error_path = session.path(error_path);
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    session
        .fs()
        .touch(session.path("src/MyLib.hs"))
        .await
        .expect("Can touch file");

    session
        .wait_for_log(BaseMatcher::span_close().in_leaf_spans(["error_log_write"]))
        .await
        .expect("ghciwatch writes ghcid.txt");
    session
        .wait_for_log("Finished running tests")
        .await
        .expect("ghciwatch runs the test suite");

    let error_contents = session
        .fs()
        .read(&error_path)
        .await
        .expect("ghciwatch writes ghcid.txt");
    expect![[r#"
        All good (3 modules)
    "#]]
    .assert_eq(&error_contents);
}
