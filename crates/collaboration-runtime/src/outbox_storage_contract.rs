//! One contract for every OutboxStorage: the same cases and assertions run
//! against MachineLocalStore (machine_local.sqlite) and, with `test-support`,
//! MemoryOutbox, so the simulation store cannot drift from the real one
//! (Lead A's call, 2026-11-18).
use crate::outbox_storage::OutboxStorage;
use crate::outbox_store::{NewOutboxItem, OutboxInsert, OutboxStoreError};
use central_link_client::{
    LinkAdmissionRefusal, MemberOutcome, OutgoingOutcome, PendingWriteState, RequestStatus,
};
use central_link_protocol::{ReplyPayload, RequestPayload};
use runtime_primitives::SyncTime;
use serde_json::json;
use workspace_model::{Caller, RequestId, WorkspaceReply, WorkspaceResult};

const MARK: i64 = 1_763_380_800_000;
const SIXTY_DAYS_MS: i64 = 60 * 24 * 60 * 60 * 1000;

fn uuid_v7(seed: u16) -> String {
    let mut random = [0_u8; 10];
    random[..2].copy_from_slice(&seed.to_be_bytes());
    uuid::Builder::from_unix_timestamp_millis(1_763_380_000_000 + u64::from(seed), &random)
        .into_uuid()
        .to_string()
}
fn caller(seed: u16) -> Caller {
    serde_json::from_value(json!({"kind": "session", "value": {
        "machineId": uuid_v7(50_000), "endpoint": "codex-local", "sessionId": format!("s{seed}")}}))
    .unwrap()
}
fn post(request: &str, workspace: u16, body: &str) -> RequestPayload {
    serde_json::from_value(json!({"kind": "workspace", "request": {
        "requestId": request, "workspaceId": uuid_v7(workspace),
        "operation": {"kind": "threadPost", "threadId": uuid_v7(60_002), "body": body, "addressees": []}}}))
    .unwrap()
}
fn item(seed: u16, body: &str, flush_at: i64) -> NewOutboxItem {
    let request = uuid_v7(seed);
    NewOutboxItem {
        request: request.parse().unwrap(),
        caller: caller(1),
        payload: post(&request, 60_001, body),
        flush_at: at(flush_at),
    }
}
fn reply() -> ReplyPayload {
    ReplyPayload::Workspace(WorkspaceReply {
        result: WorkspaceResult::Conflict,
        actors: Vec::new(),
    })
}
fn at(millis: i64) -> SyncTime {
    SyncTime::from_unix_millis(millis)
}
fn ids(items: &[&NewOutboxItem]) -> Vec<RequestId> {
    items.iter().map(|item| item.request.clone()).collect()
}

/// Stored, sent and answered, read back through every view, in order.
pub(crate) async fn lifecycle_and_views(store: &mut dyn OutboxStorage) {
    let first = item(1, "first text", MARK);
    let second = item(2, "second text", MARK + 60_000);
    assert_eq!(
        store.insert(&first, at(MARK - 2_000)).await.unwrap(),
        OutboxInsert::Stored
    );
    assert_eq!(
        store.insert(&second, at(MARK - 1_000)).await.unwrap(),
        OutboxInsert::Stored
    );
    assert_eq!(
        store.waiting().await.unwrap(),
        vec![
            (first.flush_at, first.request.clone()),
            (second.flush_at, second.request.clone())
        ]
    );
    let sent = store.mark_sent(&ids(&[&first]), at(MARK)).await.unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].payload, first.payload);
    assert_eq!(
        store
            .request_status(&first.caller, &first.request)
            .await
            .unwrap(),
        RequestStatus::Sent
    );
    assert_eq!(store.sent_without_answer().await.unwrap(), ids(&[&first]));
    store
        .record_outcome(
            &first.request,
            &MemberOutcome::Answered(reply()),
            at(MARK + 1_000),
        )
        .await
        .unwrap();

    let pending = store.pending_for(&first.caller).await.unwrap();
    let outgoing = store.outgoing_for(&first.caller, 10).await.unwrap();

    assert_eq!(
        pending
            .iter()
            .map(|write| write.request_id.clone())
            .collect::<Vec<_>>(),
        ids(&[&first, &second]),
        "pending keeps the order writes were stored"
    );
    assert_eq!(pending[0].state, PendingWriteState::Answered(reply()));
    assert!(
        matches!(&pending[1].state, PendingWriteState::Unsent { body } if body.as_str() == "second text")
    );
    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0].outcome, OutgoingOutcome::Answered(reply()));
    assert_eq!(outgoing[0].answered_at, at(MARK + 1_000));
    let views = format!("{pending:?}{outgoing:?}");
    assert!(
        !views.contains("first text"),
        "no text once answered: {views}"
    );
}

/// A resubmit replays only the same write: same caller, target and digest.
pub(crate) async fn replay_needs_the_same_caller_target_and_content(store: &mut dyn OutboxStorage) {
    let write = item(3, "posted", MARK);
    store.insert(&write, at(MARK - 1_000)).await.unwrap();
    let unanswered_again = store.insert(&write, at(MARK - 500)).await.unwrap();
    store.mark_sent(&ids(&[&write]), at(MARK)).await.unwrap();
    store
        .record_outcome(
            &write.request,
            &MemberOutcome::Answered(reply()),
            at(MARK + 1_000),
        )
        .await
        .unwrap();
    let mut other_body = write.clone();
    other_body.payload = post(write.request.as_str(), 60_001, "edited");
    let mut other_workspace = write.clone();
    other_workspace.payload = post(write.request.as_str(), 60_003, "posted");
    let mut other_caller = write.clone();
    other_caller.caller = caller(2);

    assert_eq!(
        unanswered_again,
        OutboxInsert::AlreadyStored(RequestStatus::InOutbox {
            flush_at: write.flush_at
        })
    );
    assert_eq!(
        store.insert(&write, at(MARK + 2_000)).await.unwrap(),
        OutboxInsert::AlreadyStored(RequestStatus::Answered(reply()))
    );
    for conflicting in [&other_body, &other_workspace, &other_caller] {
        assert_eq!(
            store.insert(conflicting, at(MARK + 2_000)).await.unwrap(),
            OutboxInsert::ConflictingContent
        );
    }
}

/// Unanswered sends come back as possibly-in-central; unapplied ones as unsent.
pub(crate) async fn returned_writes(store: &mut dyn OutboxStorage) {
    let lost = item(4, "lost answer", MARK);
    let revoked = item(5, "not applied", MARK);
    let refused = item(6, "refused", MARK);
    for write in [&lost, &revoked, &refused] {
        store.insert(write, at(MARK - 1_000)).await.unwrap();
    }
    store
        .mark_sent(&ids(&[&lost, &revoked, &refused]), at(MARK))
        .await
        .unwrap();

    store
        .return_unanswered(&ids(&[&lost]), at(MARK + 1_000))
        .await
        .unwrap();
    store
        .return_unapplied(&revoked.request, at(MARK + 1_000))
        .await
        .unwrap();
    store
        .record_outcome(
            &refused.request,
            &MemberOutcome::Refused(LinkAdmissionRefusal::RequestIdConflict),
            at(MARK + 1_000),
        )
        .await
        .unwrap();
    let pending = store.pending_for(&lost.caller).await.unwrap();

    assert!(
        matches!(&pending[0].state, PendingWriteState::SentAnswerLost { body } if body.as_str() == "lost answer")
    );
    assert!(
        matches!(&pending[1].state, PendingWriteState::Unsent { body } if body.as_str() == "not applied")
    );
    assert_eq!(pending.len(), 2, "a refused write is not pending");
    assert_eq!(store.waiting().await.unwrap().len(), 2);
    assert_eq!(
        store
            .request_status(&refused.caller, &refused.request)
            .await
            .unwrap(),
        RequestStatus::Refused(LinkAdmissionRefusal::RequestIdConflict)
    );
}

/// Ended writes are pruned 60 days after they ended, not a millisecond sooner.
pub(crate) async fn retention_prune(store: &mut dyn OutboxStorage) {
    let write = item(7, "old", MARK);
    let waiting = item(8, "still waiting", MARK);
    store.insert(&write, at(MARK - 1_000)).await.unwrap();
    store.insert(&waiting, at(MARK - 1_000)).await.unwrap();
    store.mark_sent(&ids(&[&write]), at(MARK)).await.unwrap();
    store
        .record_outcome(&write.request, &MemberOutcome::Answered(reply()), at(MARK))
        .await
        .unwrap();

    let early = store
        .delete_ended_before_retention(at(MARK + SIXTY_DAYS_MS - 1))
        .await
        .unwrap();
    let due = store
        .delete_ended_before_retention(at(MARK + SIXTY_DAYS_MS))
        .await
        .unwrap();

    assert_eq!((early, due), (0, 1));
    assert_eq!(
        store
            .request_status(&write.caller, &write.request)
            .await
            .unwrap(),
        RequestStatus::NotFound
    );
    assert!(matches!(
        store
            .request_status(&waiting.caller, &waiting.request)
            .await
            .unwrap(),
        RequestStatus::InOutbox { .. }
    ));
}

/// Out-of-order transitions and mismatched ids are refused; another caller
/// sees nothing.
pub(crate) async fn refusals_and_isolation(store: &mut dyn OutboxStorage) {
    let write = item(9, "mine", MARK);
    let mut mismatched = item(10, "x", MARK);
    mismatched.request = uuid_v7(11).parse().unwrap();
    store.insert(&write, at(MARK - 1_000)).await.unwrap();

    let early_outcome = store
        .record_outcome(&write.request, &MemberOutcome::Answered(reply()), at(MARK))
        .await;
    store.mark_sent(&ids(&[&write]), at(MARK)).await.unwrap();
    let sent_twice = store.mark_sent(&ids(&[&write]), at(MARK)).await;
    let mismatch = store.insert(&mismatched, at(MARK)).await;

    assert!(matches!(
        early_outcome,
        Err(OutboxStoreError::UnexpectedState { .. })
    ));
    assert!(matches!(
        sent_twice,
        Err(OutboxStoreError::UnexpectedState { .. })
    ));
    assert!(matches!(
        mismatch,
        Err(OutboxStoreError::RequestIdMismatch { .. })
    ));
    assert_eq!(
        store
            .request_status(&caller(2), &write.request)
            .await
            .unwrap(),
        RequestStatus::NotFound
    );
    assert!(store.pending_for(&caller(2)).await.unwrap().is_empty());
    assert!(store.outgoing_for(&caller(2), 10).await.unwrap().is_empty());
}

/// Runs every contract case on fresh stores from `open`.
macro_rules! outbox_storage_contract {
    ($open:expr) => {
        #[tokio::test]
        async fn lifecycle_and_views() {
            let (_root, mut store) = $open.await;
            crate::outbox_storage_contract::lifecycle_and_views(&mut store).await;
        }
        #[tokio::test]
        async fn replay_needs_the_same_caller_target_and_content() {
            let (_root, mut store) = $open.await;
            crate::outbox_storage_contract::replay_needs_the_same_caller_target_and_content(
                &mut store,
            )
            .await;
        }
        #[tokio::test]
        async fn returned_writes() {
            let (_root, mut store) = $open.await;
            crate::outbox_storage_contract::returned_writes(&mut store).await;
        }
        #[tokio::test]
        async fn retention_prune() {
            let (_root, mut store) = $open.await;
            crate::outbox_storage_contract::retention_prune(&mut store).await;
        }
        #[tokio::test]
        async fn refusals_and_isolation() {
            let (_root, mut store) = $open.await;
            crate::outbox_storage_contract::refusals_and_isolation(&mut store).await;
        }
    };
}
pub(crate) use outbox_storage_contract;

mod machine_local_store_contract {
    async fn open() -> (tempfile::TempDir, crate::MachineLocalOutbox) {
        let root = tempfile::tempdir().unwrap();
        let store = crate::MachineLocalStore::open(&root.path().join("machine_local.sqlite"))
            .await
            .unwrap();
        (root, crate::MachineLocalOutbox::new(store))
    }
    super::outbox_storage_contract!(open());
}

#[cfg(feature = "test-support")]
mod memory_outbox_contract {
    fn open() -> std::future::Ready<((), crate::MemoryOutbox)> {
        std::future::ready(((), crate::MemoryOutbox::new()))
    }
    super::outbox_storage_contract!(open());
}
