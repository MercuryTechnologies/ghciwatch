use nix::sys::signal;
use nix::sys::signal::Signal;
use nix::unistd::Pid;
use test_harness::test;
use test_harness::BaseMatcher;
use test_harness::GhciWatch;
use test_harness::JsonValue;

/// Test that `ghciwatch` can gracefully shutdown on Ctrl-C.
#[test]
async fn can_shutdown_gracefully() {
    let mut session = GhciWatch::new("tests/data/simple")
        .await
        .expect("ghciwatch starts");
    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    signal::kill(Pid::from_raw(session.pid() as i32), Signal::SIGINT)
        .expect("Failed to send Ctrl-C to ghciwatch");

    session
        .wait_for_log("^All tasks completed successfully$")
        .await
        .unwrap();

    let status = session.wait_until_exit().await.unwrap();
    assert!(status.success(), "ghciwatch exits successfully");
}

/// Test that `ghciwatch` can gracefully shutdown when the `ghci` process is unexpectedly killed.
#[test]
async fn can_shutdown_gracefully_when_ghci_killed() {
    let mut session = GhciWatch::new("tests/data/simple")
        .await
        .expect("ghciwatch starts");

    let event = session
        .wait_for_log(BaseMatcher::message("^Started ghci$"))
        .await
        .expect("ghciwatch starts ghci");
    let pid: i32 = match event.fields.get("pid").unwrap() {
        JsonValue::Number(pid) => pid,
        value => {
            panic!("pid field has wrong type: {value:?}");
        }
    }
    .as_i64()
    .expect("pid is i64")
    .try_into()
    .expect("pid is i32");

    session
        .wait_until_ready()
        .await
        .expect("ghciwatch loads ghci");

    signal::kill(Pid::from_raw(pid), Signal::SIGKILL).expect("Failed to kill ghci");

    session
        .wait_for_log("^ghci exited:")
        .await
        .expect("ghci exits");
    session.wait_for_log("^Shutdown requested$").await.unwrap();
    session
        .wait_for_log("^All tasks completed successfully$")
        .await
        .unwrap();
}
