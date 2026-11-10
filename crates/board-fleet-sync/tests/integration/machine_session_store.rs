use board_fleet_sync::MachineSessionStore;
use fleet_sync_protocol::{NoticePosition, SessionId};
use fleet_sync_session::{ClientSessionState, SessionClientStore};
use message_board_storage::FleetStore;
use std::{path::Path, sync::Arc};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

/// Opens the Router's one Fleet database the way the Host does at start: older
/// runtime files are imported once, then the session store is a view over it.
async fn open_router_fleet(
    router_directory: &Path,
) -> Result<(FleetStore, MachineSessionStore), Box<dyn std::error::Error + Send + Sync>> {
    let store = FleetStore::open(&router_directory.join(FleetStore::FILE_NAME)).await?;
    if let Err(error) = store.cache().import_legacy_runtime(router_directory).await {
        store.close().await;
        return Err(error.into());
    }
    let session = MachineSessionStore::from_cache(Arc::new(store.cache()));
    Ok((store, session))
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn reopen_retains_the_same_session_and_latest_durable_notice_cursor() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (fleet, store) = open_router_fleet(directory.path()).await?;
    assert_eq!(store.load().await?, None);
    let mut state = ClientSessionState {
        session_id: SessionId::new(uuid::Uuid::now_v7())?,
        notice_cursor: NoticePosition::new(0),
    };
    store.save(&state).await?;
    state.notice_cursor = NoticePosition::new(17);
    store.save(&state).await?;
    drop(store);
    fleet.close().await;
    let (fleet, store) = open_router_fleet(directory.path()).await?;
    assert_eq!(store.load().await?, Some(state));
    assert!(directory.path().join(FleetStore::FILE_NAME).is_file());
    assert!(!directory.path().join("machine-session/state.json").exists());
    drop(store);
    fleet.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn malformed_session_state_refuses_instead_of_minting_another_identity() -> TestResult {
    let directory = tempfile::tempdir()?;
    tokio::fs::create_dir_all(directory.path().join("machine-session")).await?;
    let path = directory.path().join("machine-session/state.json");
    tokio::fs::write(&path, b"{truncated").await?;
    assert!(open_router_fleet(directory.path()).await.is_err());
    assert_eq!(tokio::fs::read(&path).await?, b"{truncated");
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture failures propagate."
)]
async fn legacy_session_is_imported_once_and_the_file_is_removed() -> TestResult {
    let directory = tempfile::tempdir()?;
    let legacy = directory.path().join("machine-session/state.json");
    tokio::fs::create_dir_all(legacy.parent().ok_or("legacy parent")?).await?;
    let state = ClientSessionState {
        session_id: SessionId::new(uuid::Uuid::now_v7())?,
        notice_cursor: NoticePosition::new(19),
    };
    tokio::fs::write(&legacy, serde_json::to_vec(&state)?).await?;
    let (fleet, store) = open_router_fleet(directory.path()).await?;
    assert_eq!(store.load().await?, Some(state));
    assert!(!legacy.exists());
    drop(store);
    fleet.close().await;
    let (fleet, reopened) = open_router_fleet(directory.path()).await?;
    assert_eq!(
        reopened.load().await?.ok_or("stored state")?.notice_cursor,
        NoticePosition::new(19)
    );
    drop(reopened);
    fleet.close().await;
    Ok(())
}
