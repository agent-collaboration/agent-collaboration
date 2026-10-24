#![allow(clippy::panic_in_result_fn)]
use board_fleet_sync::MachineSessionStore;
use fleet_sync_protocol::{NoticePosition, SessionId};
use fleet_sync_session::{ClientSessionState, SessionClientStore};
use std::os::unix::fs::PermissionsExt;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
async fn reopen_retains_the_same_session_and_latest_durable_notice_cursor() -> TestResult {
    let directory = tempfile::tempdir()?;
    let store = MachineSessionStore::open(directory.path()).await?;
    assert_eq!(store.load().await?, None);
    let mut state = ClientSessionState {
        session_id: SessionId::new(uuid::Uuid::now_v7())?,
        notice_cursor: NoticePosition::new(0),
    };
    store.save(&state).await?;
    state.notice_cursor = NoticePosition::new(17);
    store.save(&state).await?;
    drop(store);
    let store = MachineSessionStore::open(directory.path()).await?;
    assert_eq!(store.load().await?, Some(state));
    let path = directory.path().join("machine-session/state.json");
    assert_eq!(
        tokio::fs::metadata(path).await?.permissions().mode() & 0o777,
        0o600
    );
    let mut entries = tokio::fs::read_dir(directory.path().join("machine-session")).await?;
    let mut count = 0;
    while entries.next_entry().await?.is_some() {
        count += 1;
    }
    assert_eq!(
        count, 1,
        "completed saves leave only the durable session state"
    );
    Ok(())
}

#[tokio::test]
async fn malformed_session_state_refuses_instead_of_minting_another_identity() -> TestResult {
    let directory = tempfile::tempdir()?;
    let store = MachineSessionStore::open(directory.path()).await?;
    let path = directory.path().join("machine-session/state.json");
    tokio::fs::write(&path, b"{truncated").await?;
    assert!(store.load().await.is_err());
    assert_eq!(tokio::fs::read(&path).await?, b"{truncated");
    Ok(())
}
