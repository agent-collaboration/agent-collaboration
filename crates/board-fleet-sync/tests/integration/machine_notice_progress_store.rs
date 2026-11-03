use board_fleet_sync::{MachineNoticeProgressStore, MachineSessionStore};
use fleet_sync_protocol::{NoticePosition, SessionId};
use fleet_sync_session::{ClientSessionState, SessionClientStore, StoreError};
use std::os::unix::fs::PermissionsExt;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn cold_reopen_retains_machine_progress_without_changing_transport_state() -> TestResult {
    let directory = tempfile::tempdir()?;
    let transport = MachineSessionStore::open(directory.path()).await?;
    let session = ClientSessionState {
        session_id: SessionId::new(uuid::Uuid::now_v7())?,
        notice_cursor: NoticePosition::new(3),
    };
    transport.save(&session).await?;
    let state_path = directory.path().join("machine-session/state.json");
    let original_session_bytes = tokio::fs::read(&state_path).await?;
    let progress = MachineNoticeProgressStore::open(directory.path()).await?;
    assert_eq!(progress.load().await?, NoticePosition::new(0));

    progress.save(NoticePosition::new(73)).await?;
    drop(progress);
    let reopened = MachineNoticeProgressStore::open(directory.path()).await?;

    assert_eq!(reopened.load().await?, NoticePosition::new(73));
    assert_eq!(transport.load().await?, Some(session));
    assert_eq!(tokio::fs::read(&state_path).await?, original_session_bytes);
    let progress_path = directory
        .path()
        .join("machine-session/machine-notice-progress.json");
    let stored: serde_json::Value =
        serde_json::from_slice(&tokio::fs::read(&progress_path).await?)?;
    assert_eq!(stored, serde_json::json!({"handledThrough":73}));
    assert_eq!(
        tokio::fs::metadata(progress_path)
            .await?
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn corrupt_progress_refuses_without_rewriting_or_resetting_the_file() -> TestResult {
    let directory = tempfile::tempdir()?;
    let store = MachineNoticeProgressStore::open(directory.path()).await?;
    let path = directory
        .path()
        .join("machine-session/machine-notice-progress.json");
    for invalid in [
        "{truncated",
        "{}",
        "{\"handledThrough\":-1}",
        "{\"handledThrough\":1.5}",
        "{\"handledThrough\":\"73\"}",
        "{\"handled_through\":73}",
        "{\"handledThrough\":73,\"transportCursor\":3}",
    ] {
        tokio::fs::write(&path, invalid).await?;
        assert!(
            matches!(store.load().await, Err(StoreError::Corrupt { field }) if field == "machineNoticeProgress")
        );
        assert_eq!(tokio::fs::read(&path).await?, invalid.as_bytes());
    }
    Ok(())
}
