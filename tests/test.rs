use expect_test::expect;
use test_harness::fs;
use test_harness::test;
use test_harness::GhciWatchBuilder;
use test_harness::Matcher;

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

    fs::touch(session.path("src/MyLib.hs"))
        .await
        .expect("Can touch file");

    session
        .assert_logged(Matcher::span_close().in_span("error_log_write"))
        .await
        .expect("ghciwatch writes ghcid.txt");
    session
        .assert_logged("Finished running tests")
        .await
        .expect("ghciwatch runs the test suite");

    let error_contents = fs::read(&error_path)
        .await
        .expect("ghciwatch writes ghcid.txt");
    expect![[r#"
        All good (4 modules)
    "#]]
    .assert_eq(&error_contents);
}
