//! One contract for every OutboxStorage: the same cases and assertions run
//! against MachineLocalStore (machine_local.sqlite) and, with `test-support`,
//! MemoryOutbox, so the simulation store cannot drift from the real one
//! (Lead A's call, 2026-11-18).
use crate::outbox_storage::OutboxStorage;
use crate::outbox_store::{NewOutboxItem, OutboxInsert, OutboxStoreError};
use central_link_client::{
    LinkAdmissionRefusal, MemberOutcome, OutboxCancellation, OutgoingOutcome, PendingWriteState,
    RequestStatus,
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
/// A direct message: 15 minutes from its creation stamp (outbox_deadline).
fn message(seed: u16, body: &str, flush_at: i64) -> NewOutboxItem {
    let request = uuid_v7(seed);
    NewOutboxItem {
        request: request.parse().unwrap(),
        caller: caller(1),
        payload: serde_json::from_value(json!({"kind": "mail", "request": {
            "requestId": request, "operation": {"kind": "sendMessage",
            "to": {"machineId": uuid_v7(50_001), "endpoint": "codex-local", "sessionId": "r1"},
            "body": body, "delivery": "auto", "replyTo": null,
            "sentAt": "2030-01-01T00:00:00Z"}}}))
        .unwrap(),
        flush_at: at(flush_at),
    }
}
const FIFTEEN_MINUTES_MS: i64 = 15 * 60 * 1000;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
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

/// Writes stored at the same millisecond order by request id, as SQLite
/// does, whatever order they arrived in (step-8 Verifier's probe).
pub(crate) async fn same_millisecond_writes_order_by_request_id(store: &mut dyn OutboxStorage) {
    let higher = item(41, "higher id first", MARK);
    let lower = item(40, "lower id second", MARK);
    store.insert(&higher, at(MARK - 1_000)).await.unwrap();
    store.insert(&lower, at(MARK - 1_000)).await.unwrap();

    let pending = store.pending_for(&higher.caller).await.unwrap();
    let claimed = store
        .claim_next_batch(at(MARK), 64, at(MARK))
        .await
        .unwrap()
        .writes;

    assert_eq!(
        pending
            .iter()
            .map(|write| write.request_id.clone())
            .collect::<Vec<_>>(),
        ids(&[&lower, &higher])
    );
    assert_eq!(
        claimed
            .iter()
            .map(|write| write.request.clone())
            .collect::<Vec<_>>(),
        ids(&[&lower, &higher])
    );
}

/// A claim takes only writes due by its mark, oldest first, at most `limit`,
/// and they are sent; the earliest waiting mark follows what is left.
pub(crate) async fn claim_takes_due_writes_oldest_first_up_to_the_limit(
    store: &mut dyn OutboxStorage,
) {
    let first = item(42, "first", MARK);
    let second = item(43, "second", MARK);
    let third = item(44, "third", MARK);
    let later = item(45, "next mark", MARK + 60_000);
    for (offset, write) in [&first, &second, &third, &later].into_iter().enumerate() {
        let stored_at = MARK - 10_000 + i64::try_from(offset).unwrap();
        store.insert(write, at(stored_at)).await.unwrap();
    }
    let earliest_before = store.earliest_waiting().await.unwrap();

    let claimed = store
        .claim_next_batch(at(MARK), 2, at(MARK))
        .await
        .unwrap()
        .writes;
    let earliest_after = store.earliest_waiting().await.unwrap();
    let rest = store
        .claim_next_batch(at(MARK), 64, at(MARK))
        .await
        .unwrap()
        .writes;

    assert_eq!(earliest_before, Some(at(MARK)));
    assert_eq!(
        claimed
            .iter()
            .map(|write| write.request.clone())
            .collect::<Vec<_>>(),
        ids(&[&first, &second])
    );
    assert_eq!(claimed[0].payload, first.payload);
    assert_eq!(
        store
            .request_status(&first.caller, &first.request)
            .await
            .unwrap(),
        RequestStatus::Sent
    );
    assert_eq!(earliest_after, Some(at(MARK)), "the third is still due");
    assert_eq!(
        rest.iter()
            .map(|write| write.request.clone())
            .collect::<Vec<_>>(),
        ids(&[&third]),
        "a write due on a later mark is not claimed early"
    );
    assert_eq!(
        store.earliest_waiting().await.unwrap(),
        Some(at(MARK + 60_000))
    );
}

/// A waiting write is never claimed at or past its deadline; the same claim
/// ends it, by target and by whether it had left the Mac, and drops its text.
pub(crate) async fn an_overdue_write_is_never_claimed_and_ends_with_its_reason(
    store: &mut dyn OutboxStorage,
) {
    let stored_at = MARK - 1_000;
    let deadline = stored_at + FIFTEEN_MINUTES_MS;
    let unsent = message(70, "never left", MARK);
    let lost = message(71, "answer lost", MARK);
    let post = item(72, "a thread post", MARK);
    for write in [&unsent, &lost, &post] {
        store.insert(write, at(stored_at)).await.unwrap();
    }
    store.mark_sent(&ids(&[&lost]), at(MARK)).await.unwrap();
    store
        .return_unanswered(&ids(&[&lost]), at(MARK + 1_000))
        .await
        .unwrap();

    let early = store
        .claim_next_batch(at(MARK), 1, at(deadline - 1))
        .await
        .unwrap();
    store
        .return_unanswered(&ids(&[&unsent]), at(deadline - 1))
        .await
        .unwrap();
    let claimed = store
        .claim_next_batch(at(MARK), 64, at(deadline))
        .await
        .unwrap();

    assert_eq!(
        early
            .writes
            .iter()
            .map(|w| w.request.clone())
            .collect::<Vec<_>>(),
        ids(&[&unsent]),
        "one millisecond before its deadline a direct message is still sent"
    );
    assert!(early.ended.is_empty());
    assert_eq!(
        claimed
            .writes
            .iter()
            .map(|w| w.request.clone())
            .collect::<Vec<_>>(),
        ids(&[&post]),
        "at its deadline a direct message is never claimed; the post has 24 hours"
    );
    let ended: Vec<_> = claimed
        .ended
        .iter()
        .map(|write| (write.request_id.clone(), write.caller.clone(), write.reason))
        .collect();
    assert_eq!(
        ended,
        vec![
            (
                unsent.request.clone(),
                unsent.caller.clone(),
                OutboxCancellation::NotConfirmedWithin15Minutes
            ),
            (
                lost.request.clone(),
                lost.caller.clone(),
                OutboxCancellation::NotConfirmedWithin15Minutes
            ),
        ],
        "both had left the Mac, so either may have been delivered"
    );
    let never_left = message(73, "never left", MARK);
    store.insert(&never_left, at(stored_at)).await.unwrap();
    let ended_later = store.end_overdue(at(deadline)).await.unwrap();
    assert_eq!(
        ended_later
            .iter()
            .map(|write| (write.request_id.clone(), write.reason))
            .collect::<Vec<_>>(),
        vec![(
            never_left.request.clone(),
            OutboxCancellation::NotDeliveredWithin15Minutes
        )]
    );
    assert_eq!(
        store
            .request_status(&never_left.caller, &never_left.request)
            .await
            .unwrap(),
        RequestStatus::Cancelled {
            reason: OutboxCancellation::NotDeliveredWithin15Minutes
        }
    );
    let pending = store.pending_for(&never_left.caller).await.unwrap();
    assert!(
        pending
            .iter()
            .any(|write| write.request_id == never_left.request
                && write.state
                    == PendingWriteState::Cancelled {
                        reason: OutboxCancellation::NotDeliveredWithin15Minutes
                    }),
        "the ended write reads as cancelled, with no text: {pending:?}"
    );
    assert!(matches!(
        store.insert(&never_left, at(deadline + 1)).await.unwrap(),
        OutboxInsert::AlreadyStored(RequestStatus::Cancelled { .. })
    ));
    let mut changed = never_left.clone();
    changed.payload = message(73, "different words", MARK).payload;
    assert_eq!(
        store.insert(&changed, at(deadline + 1)).await.unwrap(),
        OutboxInsert::ConflictingContent,
        "the text is gone but its digest still tells a different write"
    );
    assert!(
        store.waiting().await.unwrap().is_empty(),
        "ended writes no longer wait and the post is sent"
    );
    assert!(matches!(
        store
            .mark_sent(&ids(&[&never_left]), at(deadline + 1))
            .await,
        Err(OutboxStoreError::UnexpectedState { .. })
    ));
}

/// A sent write past its deadline ends instead of returning to waiting, so
/// a replay after a long outage never commits a second time; a write before
/// its deadline returns as before. A thread post has 24 hours.
pub(crate) async fn a_sent_write_past_its_deadline_ends_instead_of_returning(
    store: &mut dyn OutboxStorage,
) {
    let stored_at = MARK - 1_000;
    let post = item(80, "posted then lost", MARK);
    let fresh = item(81, "lost, still fresh", MARK);
    store.insert(&post, at(stored_at)).await.unwrap();
    store
        .insert(&fresh, at(stored_at + DAY_MS - 60_000))
        .await
        .unwrap();
    store.mark_sent(&ids(&[&post]), at(MARK)).await.unwrap();
    store
        .mark_sent(&ids(&[&fresh]), at(stored_at + DAY_MS - 30_000))
        .await
        .unwrap();

    let ended = store
        .return_unanswered(&ids(&[&post, &fresh]), at(stored_at + DAY_MS))
        .await
        .unwrap();
    let overdue_waiting = store.end_overdue(at(stored_at + DAY_MS)).await.unwrap();
    let claimed = store
        .claim_next_batch(at(stored_at + DAY_MS + 60_000), 64, at(stored_at + DAY_MS))
        .await
        .unwrap();

    assert_eq!(
        ended
            .iter()
            .map(|write| (write.request_id.clone(), write.reason))
            .collect::<Vec<_>>(),
        vec![(
            post.request.clone(),
            OutboxCancellation::NotConfirmedWithin24Hours
        )]
    );
    assert!(overdue_waiting.is_empty(), "nothing else is overdue");
    assert_eq!(
        store
            .request_status(&post.caller, &post.request)
            .await
            .unwrap(),
        RequestStatus::Cancelled {
            reason: OutboxCancellation::NotConfirmedWithin24Hours
        }
    );
    assert_eq!(
        claimed
            .writes
            .iter()
            .map(|w| w.request.clone())
            .collect::<Vec<_>>(),
        ids(&[&fresh]),
        "the overdue post is never resent; the fresh one is"
    );
    assert!(store.sent_without_answer().await.unwrap() == ids(&[&fresh]));
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
        #[tokio::test]
        async fn same_millisecond_writes_order_by_request_id() {
            let (_root, mut store) = $open.await;
            crate::outbox_storage_contract::same_millisecond_writes_order_by_request_id(&mut store)
                .await;
        }
        #[tokio::test]
        async fn claim_takes_due_writes_oldest_first_up_to_the_limit() {
            let (_root, mut store) = $open.await;
            crate::outbox_storage_contract::claim_takes_due_writes_oldest_first_up_to_the_limit(
                &mut store,
            )
            .await;
        }
        #[tokio::test]
        async fn an_overdue_write_is_never_claimed_and_ends_with_its_reason() {
            let (_root, mut store) = $open.await;
            crate::outbox_storage_contract::an_overdue_write_is_never_claimed_and_ends_with_its_reason(
                &mut store,
            )
            .await;
        }
        #[tokio::test]
        async fn a_sent_write_past_its_deadline_ends_instead_of_returning() {
            let (_root, mut store) = $open.await;
            crate::outbox_storage_contract::a_sent_write_past_its_deadline_ends_instead_of_returning(
                &mut store,
            )
            .await;
        }
    };
}
pub(crate) use outbox_storage_contract;
