use test_harness::fs;
use test_harness::test;
use test_harness::GhciWatchBuilder;

/// Test that `ghciwatch` can reload when a file with a `--watch-extension` is changed.
#[test]
async fn can_reload_extra_extension() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .with_args(["--watch-extension", "persistentmodels"])
        .start()
        .await
        .expect("ghciwatch starts");
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    fs::touch(session.path("src/my_model.persistentmodels"))
        .await
        .unwrap();

    session
        .wait_until_reload()
        .await
        .expect("ghciwatch reloads when a `.persistentmodels` file is created");
}
