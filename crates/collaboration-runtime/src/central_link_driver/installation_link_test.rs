//! The Host's composition before B3: the driver runs, reports not enrolled,
//! refuses immediate calls and keeps outbox writes waiting.
use super::*;
use crate::central_link_driver::driver_test_support::{
    MARK, caller, clock_at, list_areas, thread_post, uuid_v7,
};
use central_link_client::{
    LinkCalls, LinkStatus, PendingWriteState, RefusalReason, Submission, SubmitRefusal,
};

#[tokio::test]
async fn an_unconfigured_link_reports_not_enrolled_and_keeps_outbox_writes_waiting() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(Mutex::new(
        MachineLocalStore::open(&root.path().join("machine_local.sqlite"))
            .await
            .unwrap(),
    ));
    let installation = fleet_sync_protocol::PeerId::new(uuid_v7(77).parse().unwrap()).unwrap();
    let tasks = TaskTracker::new();
    let stop = CancellationToken::new();

    let calls = start_installation_link(
        &installation,
        clock_at(MARK - 20_000),
        store,
        None,
        &tasks,
        stop.clone(),
    )
    .expect("a UUIDv7 peer is a machine id");
    let status = calls.link_status().await;
    let immediate = calls.submit(caller(), list_areas()).await;
    let queued = calls.submit(caller(), thread_post(&uuid_v7(78))).await;
    let pending = calls.pending_for(caller()).await.unwrap();
    stop.cancel();
    tasks.close();
    tasks.wait().await;

    assert_eq!(
        status,
        Some(LinkStatus::Refused {
            reason: RefusalReason::CredentialRefused
        })
    );
    assert!(matches!(
        immediate,
        Submission::Refused(SubmitRefusal::NotEnrolled)
    ));
    assert!(matches!(queued, Submission::InOutbox(_)));
    assert!(matches!(
        pending.as_slice(),
        [write] if matches!(write.state, PendingWriteState::Unsent { .. })
    ));
}
