use expect_test::expect;
use test_harness::fs;
use test_harness::test;
use test_harness::GhcidNg;
use test_harness::Matcher;

/// Test that `ghcid-ng --test ...` can run a test suite.
#[test]
async fn can_run_test_suite_on_reload() {
    let error_path = "ghcid.txt";
    let mut session = GhcidNg::new_with_args(
        "tests/data/simple",
        ["--test-ghci", "TestMain.main", "--errors", error_path],
    )
    .await
    .expect("ghcid-ng starts");
    let error_path = session.path(error_path);
    session
        .wait_until_ready()
        .await
        .expect("ghcid-ng loads ghci");
    fs::touch(session.path("src/MyLib.hs"))
        .await
        .expect("Can touch file");
    session
        .get_log("Finished running tests")
        .await
        .expect("ghcid-ng runs the test suite");

    session
        .get_log(
            Matcher::span_close()
                .in_span("write")
                .in_module("ghcid_ng::ghci::stderr"),
        )
        .await
        .expect("ghcid-ng writes ghcid.txt");

    let error_contents = fs::read(&error_path)
        .await
        .expect("ghcid-ng writes ghcid.txt");

    expect![[r#"
        Ok, three modules loaded.
        0 tests executed, 0 failures :)
    "#]]
    .assert_eq(&error_contents);
}
