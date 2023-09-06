use expect_test::expect;
use indoc::indoc;

use test_harness::fs;
use test_harness::test;
use test_harness::GhcVersion::*;
use test_harness::GhcidNg;
use test_harness::Matcher;

/// Test that `ghcid-ng --errors ...` can write the error log.
#[test]
async fn can_write_error_log() {
    let error_path = "ghcid.txt";
    let mut session = GhcidNg::new_with_args("tests/data/simple", ["--errors", error_path])
        .await
        .expect("ghcid-ng starts");
    let error_path = session.path(error_path);
    session
        .wait_until_ready()
        .await
        .expect("ghcid-ng loads ghci");
    let error_contents = fs::read(&error_path)
        .await
        .expect("ghcid-ng writes ghcid.txt");
    expect![[r#"
        Ok, four modules loaded.
        Warning: No remote package servers have been specified. Usually you would have
        one specified in the config file.
    "#]]
    .assert_eq(&error_contents);
}

/// Test that `ghcid-ng --errors ...` can write compilation errors.
/// Then, test that it can reload when modules are changed and will correctly rewrite the error log
/// once it's fixed.
#[test]
async fn can_write_error_log_compilation_errors() {
    let error_path = "ghcid.txt";
    let mut session = GhcidNg::new_with_args("tests/data/simple", ["--errors", error_path])
        .await
        .expect("ghcid-ng starts");
    let error_path = session.path(error_path);
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

    let expected = match session.ghc_version() {
        Ghc90 | Ghc92 | Ghc94 => expect![[r#"
            Failed, four modules loaded.

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
        Ghc96 => expect![[r#"
            Failed, four modules loaded.

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

    fs::replace(&new_module, "myIdent = \"Uh oh!\"", "myIdent = ()")
        .await
        .unwrap();

    session
        .wait_until_add()
        .await
        .expect("ghcid-ng reloads on changes");

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
        Ok, five modules loaded.
    "#]]
    .assert_eq(&error_contents);
}
