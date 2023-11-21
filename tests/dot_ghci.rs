use test_harness::test;
use test_harness::Fs;
use test_harness::GhciWatchBuilder;

use indoc::indoc;

/// Test that `ghciwatch` can run with a custom prompt in `.ghci`.
#[test]
async fn can_run_with_custom_ghci_prompt() {
    let mut session = GhciWatchBuilder::new("tests/data/simple")
        .before_start(|project| async move {
            Fs::new()
                .write(
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
        .expect("ghciwatch starts");

    session.wait_until_ready().await.unwrap();
}
