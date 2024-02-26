use indoc::indoc;

use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::GhciWatch;

/// Test that `ghciwatch` can restart `ghci` after a module is moved.
#[test]
async fn can_restart_after_module_move() {
    let mut session = GhciWatch::new("tests/data/simple")
        .await
        .expect("ghciwatch starts");
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    let module_path = session.path("src/My/Module.hs");
    session
        .fs()
        .write(
            &module_path,
            indoc!(
                "module My.Module (myIdent) where
                myIdent :: ()
                myIdent = ()
                "
            ),
        )
        .await
        .unwrap();
    session
        .wait_until_add()
        .await
        .expect("ghciwatch loads new modules");

    {
        // Rename the module and fix the module name to match the new path.
        let new_path = session.path("src/My/CoolModule.hs");
        session.fs_mut().disable_load_bearing_sleep();
        session.fs().rename(&module_path, &new_path).await.unwrap();
        session
            .fs()
            .replace(&new_path, "module My.Module", "module My.CoolModule")
            .await
            .unwrap();
        session.fs_mut().reset_load_bearing_sleep();
    }

    session
        .wait_until_restart()
        .await
        .expect("ghciwatch restarts ghci");

    session
        .wait_for_log(BaseMatcher::module_compiling("My.CoolModule"))
        .await
        .unwrap();

    session
        .wait_for_log(BaseMatcher::compilation_succeeded())
        .await
        .unwrap();
}
