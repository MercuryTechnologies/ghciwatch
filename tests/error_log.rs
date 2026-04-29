use expect_test::expect;
use indoc::indoc;

use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::GhcVersion;
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
        All good (1 module)
    "#]]
    .assert_eq(&error_contents);
}

/// Test that `ghciwatch --errors ...` can write the error log with `--repl-no-load`.
#[test]
async fn can_write_error_log_repl_no_load() {
    let error_path = "ghcid.txt";
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_args(["--errors", error_path])
        .with_cabal_arg("--repl-no-load")
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
        All good (0 modules)
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

    session
        .wait_for_log(BaseMatcher::reload_completes())
        .await
        .expect("ghciwatch finishes reloading");

    let error_contents = session
        .fs()
        .read(&error_path)
        .await
        .expect("ghciwatch writes ghcid.txt");

    expect![[r#"
            src/My/Module.hs:3:11: error: [GHC-83865]
                * Couldn't match type `[Char]' with `()'
                  Expected: ()
                    Actual: String
                * In the expression: "Uh oh!"
                  In an equation for `myIdent': myIdent = "Uh oh!"
              |
            3 | myIdent = "Uh oh!"
              |           ^^^^^^^^
        "#]]
    .assert_eq(&error_contents);

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
        All good (2 modules)
    "#]]
    .assert_eq(&error_contents);
}

/// Test that `ghciwatch --errors ...` can use the correct basename in paths in error messages.
#[test]
async fn can_adjust_error_log_paths() {
    let error_path = "ghcid.txt";
    let mut session = GhciWatchBuilder::new("tests/data/with-dep")
        .with_args(["--errors", error_path, "--watch", "simple-dep/src"])
        .with_cabal_target("simple-dep")
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
        .replace(
            session.path("simple-dep/src/SimpleDep.hs"),
            "\"depFunc\"",
            "\"depFunc",
        )
        .await
        .expect("can break simple-dep");

    session
        .wait_for_log(BaseMatcher::span_close().in_leaf_spans(["error_log_write"]))
        .await
        .expect("ghciwatch writes ghcid.txt");

    session
        .wait_for_log(BaseMatcher::reload_completes())
        .await
        .expect("ghciwatch finishes reloading");

    let error_contents = session
        .fs()
        .read(&error_path)
        .await
        .expect("ghciwatch writes ghcid.txt");

    // The path includes the path to the package:
    let expected = match session.ghc_version() {
        GhcVersion::Ghc96 | GhcVersion::Ghc98 | GhcVersion::Ghc910 => expect![[r#"
            simple-dep/src/SimpleDep.hs:4:28: error: [GHC-21231]
                lexical error in string/character literal at character '\n'
              |
            4 | depFunc = putStrLn "depFunc
              |                            ^
        "#]],
        GhcVersion::Ghc912 => expect![[r#"
            simple-dep/src/SimpleDep.hs:4:20: error: [GHC-21231]
                lexical error at character '\n'
              |
            4 | depFunc = putStrLn "depFunc
              |                    ^^^^^^^^
        "#]],
    };

    expected.assert_eq(&error_contents);
}
