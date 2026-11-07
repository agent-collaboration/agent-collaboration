use board_fleet_sync::{MachineNoticeProgressStore, MachineSessionStore};
use fleet_sync_protocol::{NoticePosition, SessionId};
use fleet_sync_session::{ClientSessionState, SessionClientStore, StoreError};
type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn cold_reopen_retains_machine_progress_without_changing_transport_state() -> TestResult {
    let directory = tempfile::tempdir()?;
    let transport = MachineSessionStore::open(directory.path()).await?;
    let session = ClientSessionState {
        session_id: SessionId::new(uuid::Uuid::now_v7())?,
        notice_cursor: NoticePosition::new(3),
    };
    transport.save(&session).await?;
    let progress = MachineNoticeProgressStore::open(directory.path()).await?;
    assert_eq!(progress.load().await?, NoticePosition::new(0));
    progress.save(NoticePosition::new(73)).await?;
    drop(progress);
    let reopened = MachineNoticeProgressStore::open(directory.path()).await?;
    assert_eq!(reopened.load().await?, NoticePosition::new(73));
    assert_eq!(transport.load().await?, Some(session));
    assert!(!directory.path().join("machine-session/state.json").exists());
    assert!(
        !directory
            .path()
            .join("machine-session/machine-notice-progress.json")
            .exists()
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn legacy_progress_is_imported_once_and_never_overwrites_a_committed_cursor() -> TestResult {
    let directory = tempfile::tempdir()?;
    let home = directory.path().join("machine-session");
    tokio::fs::create_dir(&home).await?;
    let native = ClientSessionState {
        session_id: SessionId::new(uuid::Uuid::now_v7())?,
        notice_cursor: NoticePosition::new(31),
    };
    tokio::fs::write(home.join("state.json"), serde_json::to_vec(&native)?).await?;
    let path = home.join("machine-notice-progress.json");
    tokio::fs::write(&path, br#"{"handledThrough":31}"#).await?;
    let store = MachineNoticeProgressStore::open(directory.path()).await?;
    assert_eq!(store.load().await?, NoticePosition::new(31));
    assert!(!path.exists());
    store.save(NoticePosition::new(73)).await?;
    drop(store);
    // A crash after commit and before deletion may leave the original file.
    tokio::fs::write(home.join("state.json"), serde_json::to_vec(&native)?).await?;
    tokio::fs::write(&path, br#"{"handledThrough":31}"#).await?;
    let store = MachineNoticeProgressStore::open(directory.path()).await?;
    assert_eq!(store.load().await?, NoticePosition::new(73));
    assert!(!path.exists());
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn corrupt_progress_refuses_startup_without_rewriting_or_resetting_the_file() -> TestResult {
    for invalid in [
        "{truncated",
        "{}",
        "{\"handledThrough\":-1}",
        "{\"handledThrough\":1.5}",
        "{\"handledThrough\":\"73\"}",
        "{\"handled_through\":73}",
        "{\"handledThrough\":73,\"transportCursor\":3}",
    ] {
        let directory = tempfile::tempdir()?;
        let home = directory.path().join("machine-session");
        tokio::fs::create_dir(&home).await?;
        let session = ClientSessionState {
            session_id: SessionId::new(uuid::Uuid::now_v7())?,
            notice_cursor: NoticePosition::new(0),
        };
        tokio::fs::write(home.join("state.json"), serde_json::to_vec(&session)?).await?;
        let path = home.join("machine-notice-progress.json");
        tokio::fs::write(&path, invalid).await?;
        assert!(
            matches!(MachineNoticeProgressStore::open(directory.path()).await, Err(StoreError::Corrupt { field }) if field == "machineNoticeProgress")
        );
        assert_eq!(tokio::fs::read(&path).await?, invalid.as_bytes());
    }
    Ok(())
}
