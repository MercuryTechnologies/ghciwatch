use test_harness::fs;
use test_harness::test;
use test_harness::GhcidNgBuilder;

/// Test that `ghcid-ng` can reload when a file with a `--watch-extension` is changed.
#[test]
async fn can_reload_extra_extension() {
    let mut session = GhcidNgBuilder::new("tests/data/simple")
        .with_args(["--watch-extension", "persistentmodels"])
        .start()
        .await
        .expect("ghcid-ng starts");
    session
        .wait_until_ready()
        .await
        .expect("ghcid-ng loads ghci");

    fs::touch(session.path("src/my_model.persistentmodels"))
        .await
        .unwrap();

    session
        .wait_until_reload()
        .await
        .expect("ghcid-ng reloads when a `.persistentmodels` file is created");
}
