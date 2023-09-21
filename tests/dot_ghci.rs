use test_harness::fs;
use test_harness::test;
use test_harness::GhcidNgBuilder;

use indoc::indoc;

/// Test that `ghcid-ng` can run with a custom prompt in `.ghci`.
#[test]
async fn can_run_with_custom_ghci_prompt() {
    let mut session = GhcidNgBuilder::new("tests/data/simple")
        .before_start(|project| async move {
            fs::write(
                project.join(".ghci"),
                indoc!(
                    r#"
                    :set prompt "λ "
                    :set prompt-cont "│ "
                    "#
                ),
            )
            .await?;
            Ok(())
        })
        .start()
        .await
        .expect("ghcid-ng starts");

    session.wait_until_ready().await.unwrap();
}
