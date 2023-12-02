use indoc::indoc;

use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::Fs;
use test_harness::FullGhcVersion;
use test_harness::GhcVersion;
use test_harness::GhciWatchBuilder;
use test_harness::Matcher;

/// Test that `ghciwatch` can eval commands and invalidate its cache of eval commands.
#[test]
async fn can_eval_commands() {
    let module_path = "src/MyModule.hs";
    let cmd = "-- $> example ++ example";
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_arg("--enable-eval")
        .before_start(move |path| async move {
            Fs::new()
                .append(path.join(module_path), format!("\n{cmd}\n"))
                .await
        })
        .start()
        .await
        .expect("ghciwatch starts");
    let module_path = session.path(module_path);

    session
        .wait_until_ready()
        .await
        .expect("ghciwatch didn't start in time");

    let eval_message = BaseMatcher::message(r"MyModule.hs:\d+:\d+: example \+\+ example");
    session
        .assert_logged_or_wait(&eval_message)
        .await
        .expect("ghciwatch evals commands");
    session
        .assert_logged_or_wait(
            BaseMatcher::message("Read line").with_field("line", "exampleexample"),
        )
        .await
        .expect("ghciwatch evals commands");

    // Erase the command.
    session.fs().replace(module_path, cmd, "").await.unwrap();
    session
        .wait_until_reload()
        .await
        .expect("ghciwatch reloads");

    session
        .wait_for_log(BaseMatcher::reload_completes().but_not(eval_message))
        .await
        .unwrap();
}

/// Test that `ghciwatch` can read eval commands in changed files.
/// Also test that `ghciwatch` can parse multiline eval commands.
#[test]
async fn can_load_new_eval_commands_multiline() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_arg("--enable-eval")
        .start()
        .await
        .expect("ghciwatch starts");
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch didn't start in time");

    let module_path = session.path("src/MyModule.hs");
    let cmd = indoc!(
        "
        example
            ++ example
            ++ example"
    );
    let eval_cmd = format!("{{- $>\n{cmd}\n<$ -}}");
    session
        .fs()
        .append(&module_path, format!("\n{eval_cmd}\n"))
        .await
        .unwrap();

    let eval_message =
        BaseMatcher::message(&format!(r"MyModule.hs:\d+:\d+: {}", regex::escape(cmd)));
    session
        .wait_for_log(&eval_message)
        .await
        .expect("ghciwatch evals commands");
    session
        .wait_for_log(
            BaseMatcher::message("Read line").with_field("line", r#"^"exampleexampleexample"$"#),
        )
        .await
        .expect("ghciwatch evals commands");

    // Erase the command.
    session
        .fs()
        .replace(module_path, eval_cmd, "")
        .await
        .unwrap();
    session
        .wait_until_reload()
        .await
        .expect("ghciwatch reloads");

    session
        .wait_for_log(BaseMatcher::reload_completes().but_not(eval_message))
        .await
        .unwrap();
}

/// Test that `ghciwatch` can eval commands in non-interpreted modules.
///
/// See: <https://github.com/MercuryTechnologies/ghciwatch/pull/171>
#[test]
async fn can_eval_commands_in_non_interpreted_modules() {
    if FullGhcVersion::current().unwrap().major < GhcVersion::Ghc96 {
        tracing::info!(
            "This test relies on the `-fwrite-if-simplified-core` flag, added in GHC 9.6"
        );
        return;
    }

    let module_path = "src/MyModule.hs";
    let cmd = "-- $> example ++ example";
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_arg("--enable-eval")
        .with_ghc_arg("-fwrite-if-simplified-core")
        .with_cabal_arg("--repl-no-load")
        .before_start(move |path| async move {
            Fs::new()
                .append(path.join(module_path), format!("\n{cmd}\n"))
                .await
        })
        .start()
        .await
        .expect("ghciwatch starts");
    let module_path = session.path(module_path);

    session
        .wait_until_ready()
        .await
        .expect("ghciwatch didn't start in time");

    // Touch the module so `ghci` compiles it.
    session.fs().touch(&module_path).await.unwrap();
    session
        .wait_for_log(BaseMatcher::reload_completes())
        .await
        .unwrap();

    // Restart so it loads the compiled, non-interpreted module.
    session.restart_ghciwatch().await.unwrap();

    // Touch the module so `ghciwatch` loads it.
    session.fs().touch(&module_path).await.unwrap();

    let eval_message = BaseMatcher::message(r"MyModule.hs:\d+:\d+: example \+\+ example");
    session
        .assert_logged_or_wait(&eval_message)
        .await
        .expect("ghciwatch evals commands");
    session
        .assert_logged_or_wait(
            BaseMatcher::message("Read line").with_field("line", "exampleexample"),
        )
        .await
        .expect("ghciwatch evals commands");

    session
        .wait_for_log(BaseMatcher::reload_completes().and(BaseMatcher::message("All good!")))
        .await
        .unwrap();
}
