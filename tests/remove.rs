use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::GhciWatch;

#[test]
async fn can_remove_multiple_modules_at_once() {
    let mut session = GhciWatch::new("tests/data/simple")
        .await
        .expect("ghciwatch starts");
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    session.fs_mut().disable_load_bearing_sleep();
    session
        .fs()
        .remove(session.path("src/MyLib.hs"))
        .await
        .unwrap();
    session
        .fs()
        .remove(session.path("src/MyModule.hs"))
        .await
        .unwrap();
    session.fs_mut().reset_load_bearing_sleep();

    session
        .wait_for_log(BaseMatcher::ghci_remove())
        .await
        .expect("ghciwatch reloads on changes");
    session
        .wait_for_log(BaseMatcher::compilation_succeeded())
        .await
        .expect("ghciwatch reloads successfully");
    session
        .wait_for_log(BaseMatcher::reload_completes())
        .await
        .expect("ghciwatch finishes reloading");
}
