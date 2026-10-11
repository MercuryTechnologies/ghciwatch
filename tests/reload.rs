use indoc::indoc;

use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::GhciWatch;
use test_harness::Matcher;

/// Test that `ghciwatch` can start up and then reload on changes.
#[test]
async fn can_reload() {
    let mut session = GhciWatch::new("tests/data/simple")
        .await
        .expect("ghciwatch starts");
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");
    session
        .fs()
        .append(
            session.path("src/MyLib.hs"),
            indoc!(
                "

            hello = 1 :: Integer

            "
            ),
        )
        .await
        .unwrap();
    session
        .wait_until_reload()
        .await
        .expect("ghciwatch reloads on changes");
    session
        .wait_for_log(BaseMatcher::reload_completes())
        .await
        .expect("ghciwatch finishes reloading");
}

/// Test that `ghciwatch` can reload a module that fails to compile.
#[test]
async fn can_reload_after_error() {
    let mut session = GhciWatch::new("tests/data/simple")
        .await
        .expect("ghciwatch starts");
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
        .wait_for_log(BaseMatcher::compilation_failed())
        .await
        .unwrap();

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
        .wait_for_log(BaseMatcher::compilation_succeeded())
        .await
        .unwrap();
}

/// Test that `ghciwatch` adds a new module and reloads a modified module in a single load:
/// `:add` loads all the targets, so a following `:reload` would only repeat that load (and print
/// its errors again).
#[test]
async fn can_add_and_reload_modules_at_once() {
    let mut session = GhciWatch::new("tests/data/simple")
        .await
        .expect("ghciwatch starts");
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    // Change both files at once so they're handled together. (The load-bearing sleep before the
    // first write makes sure the polling watcher notices the change to `MyLib`.)
    session
        .fs()
        .replace(
            session.path("src/MyLib.hs"),
            "example = \"example\"",
            "example = \"changed\"",
        )
        .await
        .unwrap();
    session.fs_mut().disable_load_bearing_sleep();
    session
        .fs()
        .write(
            session.path("src/My/Module.hs"),
            indoc!(
                "module My.Module (myIdent) where
                myIdent :: ()
                myIdent = ()
                "
            ),
        )
        .await
        .unwrap();
    session.fs_mut().reset_load_bearing_sleep();

    session
        .wait_for_log(
            BaseMatcher::ghci_add()
                .and(BaseMatcher::message(
                    "^Reloaded by the preceding `:add`/`:unadd`:\n",
                ))
                .and(BaseMatcher::module_compiling("My.Module"))
                .and(BaseMatcher::module_compiling("MyLib"))
                .and(BaseMatcher::compilation_succeeded())
                .and(BaseMatcher::reload_completes())
                .but_not(BaseMatcher::ghci_reload()),
        )
        .await
        .expect("ghciwatch adds and reloads modules without `:reload`");
}
