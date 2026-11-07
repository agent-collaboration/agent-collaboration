use super::*;
use crate::FleetCache;
use fleet_session::ClientSessionState;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn observer_reads_known_handled_progress_without_creating_missing_state()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Arrange: the database and session have not been created by their owner yet.
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("fleet-cache.sqlite");

    // Act / Assert: observing missing state never creates or synthesizes it.
    assert_eq!(
        observe_machine_notice_progress(&path).await?,
        MachineNoticeProgressObservation::NotYet
    );
    assert!(!path.exists());
    let cache = FleetCache::open(&path).await?;
    assert_eq!(
        observe_machine_notice_progress(&path).await?,
        MachineNoticeProgressObservation::NotYet
    );

    // Arrange: distinct transport and handled cursors catch reading the wrong one.
    cache
        .save_machine_session_state(&ClientSessionState {
            session_id: fleet_protocol::SessionId::new(uuid::Uuid::now_v7())?,
            notice_cursor: NoticePosition::new(97),
        })
        .await?;
    cache
        .save_machine_notice_progress(NoticePosition::new(73))
        .await?;

    // Act / Assert: read the committed row beside its live owning store.
    assert_eq!(
        observe_machine_notice_progress(&path).await?,
        MachineNoticeProgressObservation::Present {
            handled_through: NoticePosition::new(73),
        }
    );
    // Invalid rows must cross the same canonical decoder, without coercion.
    sqlx::query("UPDATE fleet_session SET handled_through='073'")
        .execute(&cache.database.pool)
        .await?;
    assert!(matches!(
        observe_machine_notice_progress(&path).await,
        Err(RouterLocalStorageError::InvalidRecordKey {
            field: "machineNoticeProgress"
        })
    ));
    cache.close().await;
    Ok(())
}
