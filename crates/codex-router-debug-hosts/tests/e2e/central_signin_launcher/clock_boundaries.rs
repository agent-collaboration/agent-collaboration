//! Real-child refusal boundaries use the same inclusive range as the Worker bridge.
use super::{
    TestResult,
    child_process::{LauncherChild, launcher_command},
    observation_assertions::{assert_record, assert_stamp},
    provision,
};
use collaboration_central_conformance::signin_fixtures as signed;
use collaboration_central_core::MAX_ALARM_UNIX_MILLIS;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, BufReader};

async fn invalid_initial_and_update(time_ms: i64) -> TestResult {
    let keys = signed::SignInKeys::start().await?;
    let root = tempfile::Builder::new().prefix("sil-").tempdir()?;
    let socket = root.path().join("invalid.sock");
    let mut invalid =
        launcher_command(&root.path().join("invalid"), &socket, &keys, time_ms).spawn()?;
    let mut output =
        BufReader::new(invalid.stdout.take().ok_or("initial-time stdout missing")?).lines();
    let readiness = output.next_line().await?;
    if readiness.is_some() {
        invalid.kill().await?;
        return Err("out-of-range initial time published readiness".into());
    }
    assert_eq!(invalid.wait().await?.code(), Some(1));
    assert!(!socket.exists(), "rejected initial time left a socket");

    let mut child = LauncherChild::start(
        &root.path().join("valid"),
        &root.path().join("s.sock"),
        &keys,
    )
    .await?;
    child.expect_refused(&json!({"setTimeMs":time_ms})).await?;
    // Do not reset time after the refusal: real verification and the stored
    // record timestamp expose a clock update that applied before refusing.
    let compact = signed::stamp(signed::ORGANIZATION, signed::NOW_SECONDS)?;
    provision(&child, signed::ORGANIZATION, &compact).await?;
    let observed = child.observe(signed::ORGANIZATION).await?;
    assert_stamp(
        &observed,
        &compact,
        signed::NOW_SECONDS - 60 + signed::LIFETIME_SECONDS,
    )?;
    assert_eq!(observed.records.len(), 1);
    assert_record(
        &observed,
        0,
        signed::ORGANIZATION,
        signed::ORGANIZATION,
        "organizationProvisioned",
        signed::NOW_SECONDS * 1000,
    )?;
    child.terminate_with_stdin_open().await?;
    keys.shutdown().await
}

#[tokio::test]
async fn negative_launcher_time_is_refused_at_start_and_update_without_mutation() -> TestResult {
    invalid_initial_and_update(-1).await
}

#[tokio::test]
async fn above_maximum_launcher_time_is_refused_at_start_and_update_without_mutation() -> TestResult
{
    invalid_initial_and_update(MAX_ALARM_UNIX_MILLIS + 1).await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The inclusive range must accept both real-child startup and update endpoints."
)]
#[tokio::test]
async fn launcher_time_accepts_zero_and_maximum_at_start_and_update() -> TestResult {
    let keys = signed::SignInKeys::start().await?;
    for initial in [0, MAX_ALARM_UNIX_MILLIS] {
        let root = tempfile::Builder::new().prefix("sil-").tempdir()?;
        let mut child = LauncherChild::start_at(
            &root.path().join("data"),
            &root.path().join("s.sock"),
            &keys,
            initial,
        )
        .await?;
        for target in [0, MAX_ALARM_UNIX_MILLIS] {
            assert_eq!(child.set_time_ms(target).await?, 0);
        }
        child.terminate_with_stdin_open().await?;
    }
    keys.shutdown().await
}
