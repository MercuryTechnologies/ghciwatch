use test_harness::fs;
use test_harness::test;
use test_harness::GhcidNgBuilder;

/// Test that `ghcid-ng` can start with compile errors.
#[test]
async fn can_start_with_failed_modules() {
    let _session = GhcidNgBuilder::new("tests/data/simple")
        .before_start(|path| async move {
            fs::replace(
                path.join("src/MyModule.hs"),
                "example :: String",
                "example :: ()",
            )
            .await
        })
        .start()
        .await
        .expect("ghcid-ng starts");
}
