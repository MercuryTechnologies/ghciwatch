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
    let module_path = "src/MyLib.hs";
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

    let defined_in_multiple_files =
        BaseMatcher::message("Read stderr line").with_field("line", "defined in multiple files");

    let eval_message = BaseMatcher::message(r"MyLib.hs:\d+:\d+: -- \$> example \+\+ example")
        .but_not(defined_in_multiple_files.clone());
    session
        .assert_logged_or_wait(eval_message.clone())
        .await
        .expect("ghciwatch evals commands");
    session
        .assert_logged_or_wait(
            BaseMatcher::message("Read line").with_field("line", "exampleexample"),
        )
        .await
        .expect("ghciwatch evals commands");
    session
        .assert_logged_or_wait(
            BaseMatcher::span_close()
                .in_leaf_spans(["run_ghci", "initialize"])
                .but_not(defined_in_multiple_files.clone()),
        )
        .await
        .expect("ghciwatch finishes initializing");

    // Erase the command.
    session.clear_events();
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

    let module_path = session.path("src/MyLib.hs");
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
        BaseMatcher::message(&format!(r"MyLib.hs:\d+:\d+: -- \$> {}", regex::escape(cmd)));
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
    session.clear_events();
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

    let module_path = "src/MyLib.hs";
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

    session
        .assert_logged_or_wait(
            // Adds the module succesfully.
            BaseMatcher::message("All good!")
                // Evals the command.
                .and(BaseMatcher::message(
                    r"MyLib.hs:\d+:\d+: -- \$> example \+\+ example",
                ))
                // Reads eval output.
                .and(BaseMatcher::message("Read line").with_field("line", "exampleexample"))
                // Finishes the reload.
                .and(BaseMatcher::reload_completes())
                .but_not(
                    BaseMatcher::message("Read stderr line")
                        .with_field("line", "defined in multiple files"),
                )
                .but_not(
                    BaseMatcher::message("Read stderr line")
                        .with_field("line", "module '.*' is not interpreted"),
                ),
        )
        .await
        .expect("ghciwatch evals commands");
}

/// Test that `ghciwatch` can eval commands in a module loaded at startup, and then test that it
/// can eval commands in that module a second time.
///
/// See: <https://github.com/MercuryTechnologies/ghciwatch/issues/234>
#[test]
async fn can_eval_commands_twice() {
    let module_path = "src/MyLib.hs";
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

    // Adds the module succesfully.
    let ok_reload = BaseMatcher::message("All good!")
        // Evals the command.
        .and(BaseMatcher::message(
            r"MyLib.hs:\d+:\d+: -- \$> example \+\+ example",
        ))
        // Reads eval output.
        .and(BaseMatcher::message("Read line").with_field("line", "exampleexample"))
        // Finishes the reload.
        .and(BaseMatcher::reload_completes())
        .but_not(
            BaseMatcher::message("Read stderr line")
                .with_field("line", "defined in multiple files"),
        );

    session
        .wait_until_ready()
        .await
        .expect("ghciwatch didn't start in time");

    session.clear_events();
    session.fs().touch(&module_path).await.unwrap();
    session
        .assert_logged_or_wait(ok_reload.clone())
        .await
        .expect("ghciwatch evals commands");

    session.clear_events();
    session.fs().touch(&module_path).await.unwrap();
    session
        .assert_logged_or_wait(ok_reload)
        .await
        .expect("ghciwatch evals commands");
}

/// Test that `ghciwatch` can handle file events (e.g. `--reload-glob`s) for non-Haskell files, and
/// that we don't try to evaluate/interpret these!
#[test]
async fn eval_can_handle_non_haskell_files() {
    let relative_model_path = "models/doggy.persistentmodels";
    let cmd = "-- $> example ++ example";
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_args([
            "--enable-eval",
            "--watch",
            "models",
            "--reload-glob",
            "*.persistentmodels",
        ])
        .before_start(move |path| async move {
            let fs = Fs::new();

            fs.create_dir(path.join("models")).await?;

            fs.write(path.join(relative_model_path), format!("\n{cmd}\n"))
                .await?;

            Ok(())
        })
        .start()
        .await
        .expect("ghciwatch starts");
    let model_path = session.path(relative_model_path);

    session
        .wait_until_ready()
        .await
        .expect("ghciwatch didn't start in time");

    let reload_no_eval_matcher = BaseMatcher::reload_completes()
        .and(
            BaseMatcher::span_close()
                .in_leaf_spans(["eval"])
                .in_module("ghciwatch::ghci"),
        )
        .but_not(BaseMatcher::message(&format!(
            "Loading {} in interpreted mode for eval commands",
            regex::escape(relative_model_path)
        )));

    // Make sure we can reload.
    session.clear_events();
    session.fs().touch(&model_path).await.unwrap();
    session
        .assert_logged_or_wait(reload_no_eval_matcher.clone())
        .await
        .expect("ghciwatch handles non-Haskell files in eval mode");

    // Make sure we can reload _again_ (i.e. ghciwatch didn't crash).
    session.clear_events();
    session.fs().touch(&model_path).await.unwrap();
    session
        .assert_logged_or_wait(reload_no_eval_matcher)
        .await
        .expect("ghciwatch handles non-Haskell files in eval mode");
}

/// Test that `--eval-scope changed` only runs the eval commands in changed files.
#[test]
async fn eval_scope_changed_only_runs_changed_files() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_args(["--enable-eval", "--eval-scope", "changed"])
        .before_start(|path| async move {
            let fs = Fs::new();
            fs.replace(
                path.join("my-simple-package.cabal"),
                "exposed-modules: MyLib",
                "exposed-modules: MyLib, MyAux",
            )
            .await?;
            fs.write(
                path.join("src/MyAux.hs"),
                indoc!(
                    "
                    module MyAux (aux) where

                    aux :: String
                    aux = \"aux\"

                    -- $> aux ++ aux
                    "
                ),
            )
            .await?;
            fs.append(path.join("src/MyLib.hs"), "\n-- $> example ++ example\n")
                .await
        })
        .start()
        .await
        .expect("ghciwatch starts");
    let lib_path = session.path("src/MyLib.hs");
    let aux_path = session.path("src/MyAux.hs");

    let lib_eval = BaseMatcher::message("Read line").with_field("line", "exampleexample");
    let aux_eval = BaseMatcher::message("Read line").with_field("line", "auxaux");

    session
        .wait_until_ready()
        .await
        .expect("ghciwatch didn't start in time");
    session
        .assert_logged_or_wait(
            BaseMatcher::span_close()
                .in_leaf_spans(["run_ghci", "initialize"])
                .but_not(lib_eval.clone())
                .but_not(aux_eval.clone()),
        )
        .await
        .expect("ghciwatch doesn't eval commands at startup");

    session.clear_events();
    session.fs().touch(&lib_path).await.unwrap();
    session
        .wait_for_log(
            lib_eval
                .clone()
                .and(BaseMatcher::reload_completes())
                .but_not(aux_eval.clone()),
        )
        .await
        .expect("ghciwatch only evals commands in the changed file");

    session.clear_events();
    session.fs().touch(&aux_path).await.unwrap();
    session
        .wait_for_log(
            aux_eval
                .and(BaseMatcher::reload_completes())
                .but_not(lib_eval),
        )
        .await
        .expect("ghciwatch only evals commands in the changed file");
}

/// Test that with `--eval-scope changed`, a file changed during a failed reload has its eval
/// commands run when a restart fixes the build.
#[test]
async fn eval_scope_changed_keeps_pending_evals_across_restart() {
    let module_path = "src/MyLib.hs";
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_args(["--enable-eval", "--eval-scope", "changed"])
        .before_start(move |path| async move {
            Fs::new()
                .append(path.join(module_path), "\n-- $> example ++ example\n")
                .await
        })
        .start()
        .await
        .expect("ghciwatch starts");
    let module_path = session.path(module_path);
    let cabal_path = session.path("my-simple-package.cabal");

    session
        .wait_until_ready()
        .await
        .expect("ghciwatch didn't start in time");

    // Use `LambdaCase` without enabling it, so the reload fails.
    session
        .fs()
        .replace(
            &module_path,
            "example = \"example\"",
            "example = (\\case () -> \"example\") ()",
        )
        .await
        .unwrap();
    session
        .wait_for_log(BaseMatcher::compilation_failed())
        .await
        .unwrap();

    // Enabling `LambdaCase` in the `.cabal` file restarts the session and fixes the build
    // without changing `MyLib` again.
    session
        .fs()
        .replace(
            &cabal_path,
            "default-language: Haskell2010",
            "default-language: Haskell2010\n  default-extensions: LambdaCase",
        )
        .await
        .unwrap();
    session
        .wait_until_restart()
        .await
        .expect("ghciwatch restarts");
    // Nothing is evaluated at startup with `--eval-scope changed`, so this can only be the
    // pending eval.
    session
        .wait_for_startup_log(
            BaseMatcher::message("Read line").with_field("line", "exampleexample"),
        )
        .await
        .expect("ghciwatch evals pending commands after the restart");
}
