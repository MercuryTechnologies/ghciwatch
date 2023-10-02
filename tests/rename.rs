use test_harness::fs;
use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::GhciWatch;

/// Test that `ghciwatch` can restart correctly when modules are removed and added (i.e., renamed)
/// at the same time.
#[test]
async fn can_compile_renamed_module() {
    let mut session = GhciWatch::new("tests/data/simple")
        .await
        .expect("ghciwatch starts");
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    let module_path = session.path("src/MyModule.hs");
    let new_module_path = session.path("src/MyCoolModule.hs");
    fs::rename(&module_path, &new_module_path).await.unwrap();

    session
        .wait_until_restart()
        .await
        .expect("ghciwatch restarts on module move");

    session
        .wait_for_log(BaseMatcher::compilation_failed())
        .await
        .unwrap();

    fs::replace(new_module_path, "module MyModule", "module MyCoolModule")
        .await
        .unwrap();

    session
        .wait_until_reload()
        .await
        .expect("ghciwatch reloads on module change");

    session
        .wait_for_log(BaseMatcher::compilation_succeeded())
        .await
        .unwrap();
}
