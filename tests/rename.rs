use test_harness::fs;
use test_harness::test;
use test_harness::GhcidNg;
use test_harness::Matcher;

/// Test that `ghcid-ng` can restart correctly when modules are removed and added (i.e., renamed)
/// at the same time.
#[test]
async fn can_compile_renamed_module() {
    let mut session = GhcidNg::new("tests/data/simple")
        .await
        .expect("ghcid-ng starts");
    session
        .wait_until_ready()
        .await
        .expect("ghcid-ng loads ghci");

    let module_path = session.path("src/MyModule.hs");
    let new_module_path = session.path("src/MyCoolModule.hs");
    fs::rename(&module_path, &new_module_path).await.unwrap();

    session
        .wait_until_restart()
        .await
        .expect("ghcid-ng restarts on module move");

    session
        .assert_logged(Matcher::message("Compilation failed").in_span("reload"))
        .await
        .unwrap();

    fs::replace(new_module_path, "module MyModule", "module MyCoolModule")
        .await
        .unwrap();

    session
        .wait_until_reload()
        .await
        .expect("ghcid-ng reloads on module change");

    session
        .assert_logged(Matcher::message("Compilation succeeded").in_span("reload"))
        .await
        .unwrap();
}
