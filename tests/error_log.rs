use expect_test::expect;
use indoc::indoc;

use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::GhcVersion::*;
use test_harness::GhciWatchBuilder;

/// Test that `ghciwatch --errors ...` can write the error log.
#[test]
async fn can_write_error_log() {
    let error_path = "ghcid.txt";
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_args(["--errors", error_path])
        .start()
        .await
        .expect("ghciwatch starts");
    let error_path = session.path(error_path);
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");
    let error_contents = session
        .fs()
        .read(&error_path)
        .await
        .expect("ghciwatch writes ghcid.txt");
    expect![[r#"
        All good (4 modules)
    "#]]
    .assert_eq(&error_contents);
}

/// Test that `ghciwatch --errors ...` can write compilation errors.
/// Then, test that it can reload when modules are changed and will correctly rewrite the error log
/// once it's fixed.
#[test]
async fn can_write_error_log_compilation_errors() {
    let error_path = "ghcid.txt";
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_args(["--errors", error_path])
        .start()
        .await
        .expect("ghciwatch starts");
    let error_path = session.path(error_path);
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    let new_module = session.path("src/My/Module.hs");

    session
        .fs()
        .write(
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
        .wait_for_log(BaseMatcher::span_close().in_leaf_spans(["error_log_write"]))
        .await
        .expect("ghciwatch writes ghcid.txt");

    let error_contents = session
        .fs()
        .read(&error_path)
        .await
        .expect("ghciwatch writes ghcid.txt");

    let expected = match session.ghc_version() {
        Ghc90 | Ghc92 | Ghc94 => expect![[r#"
            src/My/Module.hs:3:11: error:
                * Couldn't match type `[Char]' with `()'
                  Expected: ()
                    Actual: String
                * In the expression: "Uh oh!"
                  In an equation for `myIdent': myIdent = "Uh oh!"
              |
            3 | myIdent = "Uh oh!"
              |           ^^^^^^^^
        "#]],
        Ghc96 | Ghc98 => expect![[r#"
            src/My/Module.hs:3:11: error: [GHC-83865]
                * Couldn't match type `[Char]' with `()'
                  Expected: ()
                    Actual: String
                * In the expression: "Uh oh!"
                  In an equation for `myIdent': myIdent = "Uh oh!"
              |
            3 | myIdent = "Uh oh!"
              |           ^^^^^^^^
        "#]],
    };

    expected.assert_eq(&error_contents);

    session
        .fs()
        .replace(&new_module, "myIdent = \"Uh oh!\"", "myIdent = ()")
        .await
        .unwrap();

    session
        .wait_until_reload()
        .await
        .expect("ghciwatch reloads on changes");

    session
        .wait_for_log(BaseMatcher::span_close().in_leaf_spans(["error_log_write"]))
        .await
        .expect("ghciwatch writes ghcid.txt");

    let error_contents = session
        .fs()
        .read(&error_path)
        .await
        .expect("ghciwatch writes ghcid.txt");

    expect![[r#"
        All good (5 modules)
    "#]]
    .assert_eq(&error_contents);
}
