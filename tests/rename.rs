use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::GhciWatch;
use test_harness::Matcher;

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
    session
        .fs()
        .rename(&module_path, &new_module_path)
        .await
        .unwrap();

    session
        .wait_for_log(BaseMatcher::ghci_add().and(BaseMatcher::ghci_remove()))
        .await
        .expect("ghciwatch adds and removes modules on module move");

    // Weirdly GHCi is fine with modules that don't match the file name as long as you specify the
    // module by path and not by name.
    session
        .wait_for_log(BaseMatcher::compilation_succeeded())
        .await
        .unwrap();

    session
        .fs()
        .replace(new_module_path, "module MyModule", "module MyCoolModule")
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
