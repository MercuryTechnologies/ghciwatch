use indoc::indoc;

use test_harness::fs;
use test_harness::test;
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
            fs::append(path.join(module_path), format!("\n{cmd}\n")).await
        })
        .start()
        .await
        .expect("ghciwatch starts");
    let module_path = session.path(module_path);

    session
        .wait_until_started()
        .await
        .expect("ghciwatch didn't start in time");

    let eval_message = Matcher::message(r"MyModule.hs:\d+:\d+: example \+\+ example");
    session
        .assert_logged(&eval_message)
        .await
        .expect("ghciwatch evals commands");
    session
        .assert_logged(Matcher::message("Read line").with_field("line", "exampleexample"))
        .await
        .expect("ghciwatch evals commands");

    // Erase the command.
    fs::replace(module_path, cmd, "").await.unwrap();
    session
        .wait_until_reload()
        .await
        .expect("ghciwatch reloads");

    session
        .assert_not_logged(
            &eval_message,
            Matcher::span_close()
                .in_span("reload")
                .in_module("ghciwatch::ghci"),
        )
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
    fs::append(&module_path, format!("\n{eval_cmd}\n"))
        .await
        .unwrap();

    let eval_message = Matcher::message(&format!(r"MyModule.hs:\d+:\d+: {}", regex::escape(cmd)));
    session
        .assert_logged(&eval_message)
        .await
        .expect("ghciwatch evals commands");
    session
        .assert_logged(
            Matcher::message("Read line").with_field("line", r#"^"exampleexampleexample"$"#),
        )
        .await
        .expect("ghciwatch evals commands");

    // Erase the command.
    fs::replace(module_path, eval_cmd, "").await.unwrap();
    session
        .wait_until_reload()
        .await
        .expect("ghciwatch reloads");

    session
        .assert_not_logged(
            &eval_message,
            Matcher::span_close()
                .in_span("reload")
                .in_module("ghciwatch::ghci"),
        )
        .await
        .unwrap();
}
