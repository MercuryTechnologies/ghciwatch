use indoc::indoc;
use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::Fs;
use test_harness::GhcVersion;
use test_harness::GhciWatchBuilder;
use test_harness::Matcher;

#[test]
async fn can_remove_multiple_modules_at_once() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .before_start(|project| async move {
            let fs = Fs::new();

            fs.replace(
                project.join("my-simple-package.cabal"),
                "exposed-modules: MyLib",
                "exposed-modules: MyLib, MyModule",
            )
            .await?;

            fs.write(
                project.join("src/MyModule.hs"),
                indoc!(
                    r#"
                    module MyModule (myFunc) where

                    myFunc :: IO ()
                    myFunc = putStrLn "hello!"
                    "#
                ),
            )
            .await?;

            Ok(())
        })
        .start()
        .await
        .expect("ghciwatch starts");

    session
        .wait_for_log(
            BaseMatcher::message("Read line")
                .in_spans(["refresh_targets"])
                .with_field("line", "MyLib")
                .and(
                    BaseMatcher::message("Read line")
                        .in_spans(["refresh_targets"])
                        .with_field("line", "MyModule"),
                ),
        )
        .await
        .expect("2 modules are loaded");

    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    session.fs_mut().disable_load_bearing_sleep();
    session
        .fs()
        .remove(session.path("src/MyLib.hs"))
        .await
        .unwrap();
    session
        .fs()
        .remove(session.path("src/MyModule.hs"))
        .await
        .unwrap();
    session.fs_mut().reset_load_bearing_sleep();

    session
        .wait_for_log(BaseMatcher::ghci_remove())
        .await
        .expect("ghciwatch reloads on changes");
    session
        .wait_for_log(
            BaseMatcher::message("Read line")
                .in_spans(["remove_modules"])
                .with_field(
                    "line",
                    match session.ghc_version() {
                        // TODO: Why on Earth didn't I implement `PartialOrd` on `GhcVersion`?
                        GhcVersion::Ghc910 | GhcVersion::Ghc912 => "Ok, two modules unadded.",
                        // Older versions just tell us how many modules are _left_.
                        _ => "Ok, no modules loaded.",
                    },
                ),
        )
        .await
        .expect("2 modules are unloaded");
    session
        .wait_for_log(BaseMatcher::compilation_succeeded())
        .await
        .expect("ghciwatch reloads successfully");
    session
        .wait_for_log(BaseMatcher::reload_completes())
        .await
        .expect("ghciwatch finishes reloading");
}
