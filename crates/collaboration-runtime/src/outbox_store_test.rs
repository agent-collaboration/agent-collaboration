use super::*;
use central_link_client::{LinkAdmissionRefusal, PendingWriteState};
use central_link_protocol::ReplyPayload;
use hegel::{TestCase, generators as gs};
use serde_json::json;
use std::collections::BTreeMap;
use workspace_model::{PostText, WorkspaceReply, WorkspaceResult};

const MARK: i64 = 1_763_380_800_000;

fn uuid_v7(seed: u16) -> String {
    let mut random = [0_u8; 10];
    random[..2].copy_from_slice(&seed.to_be_bytes());
    uuid::Builder::from_unix_timestamp_millis(1_763_380_000_000 + u64::from(seed), &random)
        .into_uuid()
        .to_string()
}

fn caller(seed: u16) -> Caller {
    serde_json::from_value(json!({
        "kind": "session",
        "value": {"machineId": uuid_v7(60_000), "endpoint": "claude-local", "sessionId": format!("s{seed}")},
    }))
    .unwrap()
}

fn thread_post(request: &str, body: &str) -> RequestPayload {
    serde_json::from_value(json!({
        "kind": "workspace",
        "request": {
            "requestId": request,
            "workspaceId": uuid_v7(60_001),
            "operation": {"kind": "threadPost", "threadId": uuid_v7(60_002), "body": body, "addressees": []},
        },
    }))
    .unwrap()
}

fn item(seed: u16, body: &str, flush_at: i64) -> NewOutboxItem {
    let request = uuid_v7(seed);
    NewOutboxItem {
        request: request.parse().unwrap(),
        caller: caller(1),
        payload: thread_post(&request, body),
        flush_at: SyncTime::from_unix_millis(flush_at),
    }
}

fn accepted_reply() -> ReplyPayload {
    ReplyPayload::Workspace(WorkspaceReply {
        result: WorkspaceResult::Conflict,
        actors: Vec::new(),
    })
}

fn accepted() -> MemberOutcome {
    MemberOutcome::Answered(accepted_reply())
}

fn at(millis: i64) -> SyncTime {
    SyncTime::from_unix_millis(millis)
}

async fn open(root: &tempfile::TempDir) -> ProviderOperationStore {
    ProviderOperationStore::open(&root.path().join("provider-operations.sqlite"))
        .await
        .unwrap()
}

#[tokio::test]
async fn a_write_moves_in_outbox_sent_accepted_and_survives_a_reopen() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(&root).await;
    let write = item(1, "hello", MARK);

    let inserted = MacOutbox::new(&mut store)
        .insert(&write, at(MARK - 20_000))
        .await
        .unwrap();
    let in_outbox = MacOutbox::new(&mut store)
        .request_status(&write.caller, &write.request)
        .await
        .unwrap();
    let sent = MacOutbox::new(&mut store)
        .mark_sent(std::slice::from_ref(&write.request), at(MARK))
        .await
        .unwrap();
    let after_send = MacOutbox::new(&mut store)
        .request_status(&write.caller, &write.request)
        .await
        .unwrap();
    MacOutbox::new(&mut store)
        .record_outcome(&write.request, &accepted(), at(MARK + 1_000))
        .await
        .unwrap();
    store.close().await.unwrap();
    let mut reopened = open(&root).await;
    let after_reopen = MacOutbox::new(&mut reopened)
        .request_status(&write.caller, &write.request)
        .await
        .unwrap();

    assert_eq!(inserted, OutboxInsert::Stored);
    assert_eq!(in_outbox, RequestStatus::InOutbox { flush_at: at(MARK) });
    assert_eq!(
        sent,
        vec![StoredOutboxWrite {
            request: write.request.clone(),
            caller: write.caller.clone(),
            payload: write.payload.clone(),
        }]
    );
    assert_eq!(after_send, RequestStatus::Sent);
    assert_eq!(after_reopen, RequestStatus::Answered(accepted_reply()));
}

#[tokio::test]
async fn an_answer_or_refusal_drops_the_text_from_the_row_and_every_view() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(&root).await;
    let answered = item(11, "first secret text", MARK);
    let refused = item(12, "second secret text", MARK);
    for write in [&answered, &refused] {
        MacOutbox::new(&mut store)
            .insert(write, at(MARK - 20_000))
            .await
            .unwrap();
    }
    let unsent = MacOutbox::new(&mut store)
        .pending_for(&answered.caller)
        .await
        .unwrap();
    MacOutbox::new(&mut store)
        .mark_sent(
            &[answered.request.clone(), refused.request.clone()],
            at(MARK),
        )
        .await
        .unwrap();

    MacOutbox::new(&mut store)
        .record_outcome(&answered.request, &accepted(), at(MARK + 1_000))
        .await
        .unwrap();
    MacOutbox::new(&mut store)
        .record_outcome(
            &refused.request,
            &MemberOutcome::Refused(LinkAdmissionRefusal::CredentialRefused),
            at(MARK + 1_000),
        )
        .await
        .unwrap();
    let rows: Vec<String> = sqlx::query_scalar("SELECT payload_json FROM outbox_items")
        .fetch_all(&mut store.connection)
        .await
        .unwrap();
    let pending = MacOutbox::new(&mut store)
        .pending_for(&answered.caller)
        .await
        .unwrap();
    let outgoing = MacOutbox::new(&mut store)
        .outgoing_for(&answered.caller, 10)
        .await
        .unwrap();
    let resubmitted = MacOutbox::new(&mut store)
        .insert(&answered, at(MARK + 2_000))
        .await
        .unwrap();

    assert!(matches!(
        &unsent[0].state,
        PendingWriteState::Unsent { body } if body.as_str() == "first secret text"
    ));
    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert!(!row.contains("secret text"), "stored row keeps text: {row}");
    }
    let views = format!("{pending:?}{outgoing:?}");
    assert!(!views.contains("secret text"), "a view keeps text: {views}");
    assert_eq!(pending.len(), 1, "the refused write is not pending");
    assert_eq!(pending[0].target, outgoing[1].target);
    assert_eq!(outgoing.len(), 2);
    assert_eq!(
        resubmitted,
        OutboxInsert::AlreadyStored(RequestStatus::Answered(accepted_reply()))
    );
}

#[tokio::test]
async fn another_caller_sees_not_found_and_resubmits_are_idempotent_or_conflicts() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(&root).await;
    let write = item(2, "same", MARK);
    MacOutbox::new(&mut store)
        .insert(&write, at(MARK - 20_000))
        .await
        .unwrap();

    let foreign = MacOutbox::new(&mut store)
        .request_status(&caller(2), &write.request)
        .await
        .unwrap();
    let again = MacOutbox::new(&mut store)
        .insert(&write, at(MARK - 10_000))
        .await
        .unwrap();
    let changed = MacOutbox::new(&mut store)
        .insert(&item(2, "different", MARK), at(MARK - 10_000))
        .await
        .unwrap();
    let other_caller = NewOutboxItem {
        caller: caller(2),
        ..write.clone()
    };
    let stolen = MacOutbox::new(&mut store)
        .insert(&other_caller, at(MARK - 10_000))
        .await
        .unwrap();

    assert_eq!(foreign, RequestStatus::NotFound);
    assert_eq!(
        again,
        OutboxInsert::AlreadyStored(RequestStatus::InOutbox { flush_at: at(MARK) })
    );
    assert_eq!(changed, OutboxInsert::ConflictingContent);
    assert_eq!(stolen, OutboxInsert::ConflictingContent);
}

#[tokio::test]
async fn unanswered_writes_return_with_their_receipt_mark_and_refusals_are_kept() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(&root).await;
    let first = item(3, "a", MARK);
    let second = item(4, "b", MARK + 60_000);
    MacOutbox::new(&mut store)
        .insert(&first, at(MARK - 1_000))
        .await
        .unwrap();
    MacOutbox::new(&mut store)
        .insert(&second, at(MARK + 1_000))
        .await
        .unwrap();
    MacOutbox::new(&mut store)
        .mark_sent(
            &[first.request.clone(), second.request.clone()],
            at(MARK + 60_000),
        )
        .await
        .unwrap();

    let unanswered = MacOutbox::new(&mut store)
        .sent_without_answer()
        .await
        .unwrap();
    MacOutbox::new(&mut store)
        .return_unanswered(std::slice::from_ref(&first.request), at(MARK + 70_000))
        .await
        .unwrap();
    let refusal = MemberOutcome::Refused(LinkAdmissionRefusal::MachineRevoked);
    MacOutbox::new(&mut store)
        .record_outcome(&second.request, &refusal, at(MARK + 70_000))
        .await
        .unwrap();

    let mut expected_unanswered = vec![first.request.clone(), second.request.clone()];
    expected_unanswered.sort();
    assert_eq!(unanswered, expected_unanswered);
    assert_eq!(
        MacOutbox::new(&mut store).waiting().await.unwrap(),
        vec![(at(MARK), first.request.clone())]
    );
    assert_eq!(
        MacOutbox::new(&mut store)
            .request_status(&second.caller, &second.request)
            .await
            .unwrap(),
        RequestStatus::Refused(LinkAdmissionRefusal::MachineRevoked)
    );
}

#[tokio::test]
async fn out_of_order_transitions_and_mismatched_ids_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(&root).await;
    let write = item(5, "x", MARK);
    MacOutbox::new(&mut store)
        .insert(&write, at(MARK - 1_000))
        .await
        .unwrap();
    let mismatched = NewOutboxItem {
        request: uuid_v7(6).parse().unwrap(),
        ..write.clone()
    };

    let answer_unsent = MacOutbox::new(&mut store)
        .record_outcome(&write.request, &accepted(), at(MARK))
        .await;
    let return_unsent = MacOutbox::new(&mut store)
        .return_unanswered(std::slice::from_ref(&write.request), at(MARK))
        .await;
    let mismatch = MacOutbox::new(&mut store)
        .insert(&mismatched, at(MARK))
        .await;
    MacOutbox::new(&mut store)
        .mark_sent(std::slice::from_ref(&write.request), at(MARK))
        .await
        .unwrap();
    let send_twice = MacOutbox::new(&mut store)
        .mark_sent(std::slice::from_ref(&write.request), at(MARK))
        .await;

    assert!(matches!(
        answer_unsent,
        Err(OutboxStoreError::UnexpectedState { .. })
    ));
    assert!(matches!(
        return_unsent,
        Err(OutboxStoreError::UnexpectedState { .. })
    ));
    assert!(matches!(
        mismatch,
        Err(OutboxStoreError::RequestIdMismatch { .. })
    ));
    assert!(matches!(
        send_twice,
        Err(OutboxStoreError::UnexpectedState { .. })
    ));
}

#[tokio::test]
async fn an_invalid_stored_row_is_refused_not_coerced() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(&root).await;
    let write = item(7, "x", MARK);
    MacOutbox::new(&mut store)
        .insert(&write, at(MARK - 1_000))
        .await
        .unwrap();
    sqlx::query("UPDATE outbox_items SET item_state = 'delivered'")
        .execute(&mut store.connection)
        .await
        .unwrap();
    let bad_state = MacOutbox::new(&mut store)
        .request_status(&write.caller, &write.request)
        .await;
    // An answered row that still holds the write's text is refused.
    let answer = serde_json::to_string(&accepted_reply()).unwrap();
    sqlx::query("UPDATE outbox_items SET item_state = 'answered', outcome_json = ?")
        .bind(&answer)
        .execute(&mut store.connection)
        .await
        .unwrap();
    let answered_with_text = MacOutbox::new(&mut store)
        .request_status(&write.caller, &write.request)
        .await;
    let body_free = json!({"requestId": write.request.as_str(), "target": {
        "kind": "thread", "workspaceId": uuid_v7(60_001), "threadId": uuid_v7(60_002)}});
    sqlx::query("UPDATE outbox_items SET payload_json = ?, outcome_json = NULL")
        .bind(body_free.to_string())
        .execute(&mut store.connection)
        .await
        .unwrap();
    let missing_outcome = MacOutbox::new(&mut store)
        .request_status(&write.caller, &write.request)
        .await;

    assert!(matches!(
        bad_state,
        Err(OutboxStoreError::CorruptItem {
            field: "item_state",
            ..
        })
    ));
    assert!(matches!(
        answered_with_text,
        Err(OutboxStoreError::CorruptItem {
            field: "payload_json",
            ..
        })
    ));
    assert!(matches!(
        missing_outcome,
        Err(OutboxStoreError::CorruptItem {
            field: "outcome_json",
            ..
        })
    ));
}

/// A write's state in the model, as the store should report it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ModelState {
    Unsent,
    Sent,
    SentAnswerLost,
    Accepted,
    Refused,
}

#[derive(Clone, Copy, Debug)]
struct ModelWrite {
    flush_at: SyncTime,
    state: ModelState,
    answered_at: Option<i64>,
}

impl ModelWrite {
    fn status(&self) -> RequestStatus {
        match self.state {
            ModelState::Unsent | ModelState::SentAnswerLost => RequestStatus::InOutbox {
                flush_at: self.flush_at,
            },
            ModelState::Sent => RequestStatus::Sent,
            ModelState::Accepted => RequestStatus::Answered(accepted_reply()),
            ModelState::Refused => RequestStatus::Refused(LinkAdmissionRefusal::RequestIdConflict),
        }
    }
}

/// Each model write's own text, so a leftover body is attributable.
fn body_of(request_id: &str) -> String {
    format!("unsent text {request_id}")
}

fn post_text(request_id: &RequestId) -> PostText {
    PostText::try_from(body_of(request_id.as_str())).unwrap()
}

const RETENTION_MS: i64 = 60 * 24 * 60 * 60 * 1000;

/// The real SQLite store against a model of each write's state.
struct OutboxStoreModel {
    runtime: tokio::runtime::Runtime,
    _root: tempfile::TempDir,
    store: ProviderOperationStore,
    model: BTreeMap<RequestId, ModelWrite>,
    next_seed: u16,
    now: i64,
}

impl OutboxStoreModel {
    fn ids_in(&self, states: &[ModelState]) -> Vec<RequestId> {
        self.model
            .iter()
            .filter(|(_, write)| states.contains(&write.state))
            .map(|(id, _)| id.clone())
            .collect()
    }
}

#[hegel::state_machine]
impl OutboxStoreModel {
    #[rule(weight = 3)]
    fn store_a_write(&mut self, tc: TestCase) {
        tc.assume(self.next_seed < 1_000);
        let marks_ahead = tc.draw(gs::integers::<i64>().min_value(1).max_value(3));
        let body = body_of(&uuid_v7(self.next_seed));
        let write = item(self.next_seed, &body, MARK + marks_ahead * 60_000);
        self.next_seed += 1;
        let result = self
            .runtime
            .block_on(MacOutbox::new(&mut self.store).insert(&write, at(self.now)));
        assert_eq!(result.unwrap(), OutboxInsert::Stored);
        self.model.insert(
            write.request,
            ModelWrite {
                flush_at: write.flush_at,
                state: ModelState::Unsent,
                answered_at: None,
            },
        );
    }

    #[rule(weight = 2)]
    fn send_waiting_writes(&mut self, tc: TestCase) {
        let waiting = self.ids_in(&[ModelState::Unsent, ModelState::SentAnswerLost]);
        tc.assume(!waiting.is_empty());
        let count = tc.draw(
            gs::integers::<usize>()
                .min_value(1)
                .max_value(waiting.len()),
        );
        let batch = &waiting[..count];
        self.now += 1_000;
        let sent = self
            .runtime
            .block_on(MacOutbox::new(&mut self.store).mark_sent(batch, at(self.now)))
            .unwrap();
        assert_eq!(
            sent.iter()
                .map(|write| write.request.clone())
                .collect::<Vec<_>>(),
            batch
        );
        for id in batch {
            if let Some(write) = self.model.get_mut(id) {
                write.state = ModelState::Sent;
            }
        }
    }

    #[rule(weight = 2)]
    fn answer_or_return_a_sent_write(&mut self, tc: TestCase) {
        let sent = self.ids_in(&[ModelState::Sent]);
        tc.assume(!sent.is_empty());
        let id = tc.draw(gs::sampled_from(sent));
        self.now += 1_000;
        let now = self.now;
        let choice = tc.draw(gs::integers::<u8>().max_value(2));
        let mut outbox = MacOutbox::new(&mut self.store);
        let next = match choice {
            0 => {
                self.runtime
                    .block_on(outbox.return_unanswered(std::slice::from_ref(&id), at(now)))
                    .unwrap();
                ModelState::SentAnswerLost
            }
            1 => {
                let refusal = MemberOutcome::Refused(LinkAdmissionRefusal::RequestIdConflict);
                self.runtime
                    .block_on(outbox.record_outcome(&id, &refusal, at(now)))
                    .unwrap();
                ModelState::Refused
            }
            _ => {
                self.runtime
                    .block_on(outbox.record_outcome(&id, &accepted(), at(now)))
                    .unwrap();
                ModelState::Accepted
            }
        };
        if let Some(write) = self.model.get_mut(&id) {
            write.state = next;
            if matches!(next, ModelState::Accepted | ModelState::Refused) {
                write.answered_at = Some(now);
            }
        }
    }

    #[rule]
    fn time_passes_and_answered_writes_age_out(&mut self, tc: TestCase) {
        self.now += tc.draw(
            gs::integers::<i64>()
                .min_value(0)
                .max_value(2 * RETENTION_MS),
        );
        let deleted = self
            .runtime
            .block_on(
                MacOutbox::new(&mut self.store).delete_answered_before_retention(at(self.now)),
            )
            .unwrap();
        let cutoff = self.now - RETENTION_MS;
        let before = self.model.len();
        self.model
            .retain(|_, write| write.answered_at.is_none_or(|answered| answered > cutoff));
        assert_eq!(usize::try_from(deleted).unwrap(), before - self.model.len());
    }

    #[invariant]
    fn the_store_agrees_with_the_model(&mut self, _: TestCase) {
        for (id, write) in &self.model {
            let stored = self
                .runtime
                .block_on(MacOutbox::new(&mut self.store).request_status(&caller(1), id));
            assert_eq!(stored.unwrap(), write.status());
        }
        let mut expected_waiting: Vec<(SyncTime, RequestId)> = self
            .model
            .iter()
            .filter(|(_, write)| {
                matches!(write.state, ModelState::Unsent | ModelState::SentAnswerLost)
            })
            .map(|(id, write)| (write.flush_at, id.clone()))
            .collect();
        expected_waiting.sort();
        assert_eq!(
            self.runtime
                .block_on(MacOutbox::new(&mut self.store).waiting())
                .unwrap(),
            expected_waiting
        );
        let expected_pending: Vec<(RequestId, PendingWriteState)> = self
            .model
            .iter()
            .filter_map(|(id, write)| {
                let state = match write.state {
                    ModelState::Unsent => PendingWriteState::Unsent {
                        body: post_text(id),
                    },
                    ModelState::Sent | ModelState::SentAnswerLost => {
                        PendingWriteState::SentAnswerLost {
                            body: post_text(id),
                        }
                    }
                    ModelState::Accepted => PendingWriteState::Answered(accepted_reply()),
                    ModelState::Refused => return None,
                };
                Some((id.clone(), state))
            })
            .collect();
        let pending = self
            .runtime
            .block_on(MacOutbox::new(&mut self.store).pending_for(&caller(1)))
            .unwrap();
        assert_eq!(
            pending
                .into_iter()
                .map(|write| (write.request_id, write.state))
                .collect::<Vec<_>>(),
            expected_pending
        );
        let mut expected_outgoing: Vec<(i64, RequestId)> = self
            .model
            .iter()
            .filter_map(|(id, write)| write.answered_at.map(|at| (at, id.clone())))
            .collect();
        expected_outgoing.sort_by(|left, right| right.cmp(left));
        let outgoing = self
            .runtime
            .block_on(MacOutbox::new(&mut self.store).outgoing_for(&caller(1), u16::MAX))
            .unwrap();
        assert_eq!(
            outgoing
                .into_iter()
                .map(|write| (write.answered_at.unix_millis(), write.request_id))
                .collect::<Vec<_>>(),
            expected_outgoing
        );
        // Owner, 2026-11-15: unsent text stays on the Mac only until central
        // answers. An answered or refused row keeps no text, in the row itself.
        for (id, write) in &self.model {
            if !matches!(write.state, ModelState::Accepted | ModelState::Refused) {
                continue;
            }
            let stored: String = self
                .runtime
                .block_on(
                    sqlx::query_scalar(
                        "SELECT payload_json FROM outbox_items WHERE request_id = ?",
                    )
                    .bind(id.as_str())
                    .fetch_one(&mut self.store.connection),
                )
                .unwrap();
            assert!(
                !stored.contains(&body_of(id.as_str())),
                "answered row {id} still holds its text"
            );
        }
        let foreign = self
            .runtime
            .block_on(MacOutbox::new(&mut self.store).pending_for(&caller(2)))
            .unwrap();
        assert!(
            foreign.is_empty(),
            "another caller sees none of these writes"
        );
    }
}

#[hegel::test(test_cases = 25)]
fn the_outbox_store_tracks_every_write_like_its_model(tc: TestCase) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let store = runtime.block_on(open(&root));
    let machine = OutboxStoreModel {
        runtime,
        _root: root,
        store,
        model: BTreeMap::new(),
        next_seed: 0,
        now: MARK - 30_000,
    };
    hegel::stateful::machine(machine).run(tc);
}
