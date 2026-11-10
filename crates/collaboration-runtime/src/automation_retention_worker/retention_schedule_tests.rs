//! The retention worker on real stores: it arms one timer at the first moment a delete rule
//! allows a row to go, deletes nothing before it, and a commit that makes nothing due
//! deletes nothing.
//!
//! Real store operations remain asynchronous; only the manual clock advances deadlines.
use super::retention_test_harness::*;
use crate::LoopSnapshot;
use agent_automation::{InstructionText, OperationId};
use automation_storage::InstructionUpdate;
use chrono::{DateTime, Utc};
use runtime_primitives::Clock;
use sqlx::Connection;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

async fn assert_deleted_at_its_cutoff(stamped: &str, wall: &str, expected_due: &str) {
    let directory = tempfile::tempdir().unwrap();
    let store = open_store(directory.path()).await;
    let mut writer = fixture_writer(directory.path()).await;
    insert_event(&mut writer, "aging", at(stamped)).await;

    let worker = RunningWorker::start(&store, at(wall)).await;

    let parked = worker.snapshot();
    assert_eq!(
        armed(&parked),
        Some(at(expected_due)),
        "one timer at the strict cutoff"
    );
    assert!(
        prunes(&parked).is_empty(),
        "nothing was due before the cutoff"
    );
    assert_eq!(event_ids(&mut writer).await, vec!["aging"]);

    run_to_the_armed_time(&worker).await;

    let woke = worker.snapshot();
    let woke_at = DateTime::from_timestamp_millis(worker.clock.now().unix_millis()).unwrap();
    assert!(
        woke_at >= at(expected_due)
            && woke_at - at(expected_due) <= chrono::Duration::milliseconds(2),
        "the clock reached the cutoff and no further: {woke_at}"
    );
    assert_eq!(prunes(&woke), vec!["retention.events.prune"]);
    assert!(event_ids(&mut writer).await.is_empty());
    assert_eq!(
        armed(&woke),
        None,
        "with nothing left to age, no timer is held"
    );
    worker.stop().await;
}

#[tokio::test]
async fn a_row_goes_one_millisecond_after_exactly_two_calendar_months() {
    assert_deleted_at_its_cutoff(
        "2026-01-15T10:00:05.000Z",
        "2026-03-15T10:00:00.000Z",
        "2026-03-15T10:00:05.001Z",
    )
    .await;
}

#[tokio::test]
async fn a_month_end_row_waits_until_the_cutoff_leaves_the_short_month() {
    // The cutoff stands at 2025-12-28 for the whole of 2026-02-28.
    assert_deleted_at_its_cutoff(
        "2025-12-31T12:00:00.000Z",
        "2026-02-28T22:00:00.000Z",
        "2026-03-01T00:00:00.000Z",
    )
    .await;
}

#[tokio::test]
async fn a_protected_old_revision_holds_no_timer_and_goes_when_its_protection_is_removed() {
    let directory = tempfile::tempdir().unwrap();
    let store = open_store(directory.path()).await;
    let wall = at("2026-06-15T12:00:00.000Z");
    // The instruction's first revision is long past retention, but it is the current one.
    let instruction = store
        .lock()
        .await
        .create_instruction(
            &OperationId::generate(),
            &InstructionText::try_from("first revision".to_owned()).unwrap(),
            at("2026-01-01T00:00:00.000Z").timestamp_millis(),
        )
        .await
        .unwrap();
    let mut writer = fixture_writer(directory.path()).await;
    insert_event(&mut writer, "later", at("2026-04-15T13:00:00.000Z")).await;

    let worker = RunningWorker::start(&store, wall).await;

    let parked = worker.snapshot();
    assert_eq!(
        revisions(&mut writer).await,
        1,
        "the current revision stays"
    );
    assert_eq!(
        armed(&parked),
        Some(at("2026-06-15T13:00:00.001Z")),
        "the protected revision does not arm the timer; the later event does"
    );
    let passes = parked.passes;

    // Replacing the revision is the commit that removes its protection.
    store
        .lock()
        .await
        .update_instruction(&InstructionUpdate {
            operation_id: OperationId::generate(),
            instruction_id: instruction.instruction_id.clone(),
            expected_revision_id: instruction.revision_id.clone(),
            text: InstructionText::try_from("second revision".to_owned()).unwrap(),
            now_ms: wall.timestamp_millis(),
        })
        .await
        .unwrap();
    worker.observation.asleep_at_least(parked.sleeps + 1).await;

    let woke = worker.snapshot();
    assert!(woke.passes > passes, "the commit woke the worker");
    let since_park = LoopSnapshot {
        reads: woke.reads[parked.reads.len()..].to_vec(),
        ..LoopSnapshot::default()
    };
    assert_eq!(
        prunes(&since_park),
        vec!["retention.instruction_revisions.prune"],
        "the old revision went at its due time, which had already passed"
    );
    assert_eq!(
        revisions(&mut writer).await,
        1,
        "only the new revision is left"
    );
    assert_eq!(armed(&woke), Some(at("2026-06-15T13:00:00.001Z")));
    worker.stop().await;
}

#[tokio::test]
async fn a_commit_that_makes_nothing_due_reads_due_times_and_deletes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let store = open_store(directory.path()).await;
    let wall = at("2026-06-15T12:00:00.000Z");
    let mut writer = fixture_writer(directory.path()).await;
    insert_event(&mut writer, "later", at("2026-04-15T13:00:00.000Z")).await;
    let worker = RunningWorker::start(&store, wall).await;
    let parked = worker.snapshot();

    // A new instruction commits fresh rows, none of them old enough to go.
    store
        .lock()
        .await
        .create_instruction(
            &OperationId::generate(),
            &InstructionText::try_from("fresh".to_owned()).unwrap(),
            wall.timestamp_millis(),
        )
        .await
        .unwrap();
    worker.observation.asleep_at_least(parked.sleeps + 1).await;

    let woke = worker.snapshot();
    let since_park = &woke.reads[parked.reads.len()..];
    assert!(
        !since_park.is_empty() && since_park.len() % 6 == 0,
        "whole passes of one due read per automation class: {since_park:?}"
    );
    assert!(
        since_park.iter().all(|read| read.ends_with(".due")),
        "a commit that leaves no class due runs no DELETE: {since_park:?}"
    );
    assert_eq!(armed(&woke), armed(&parked));
    assert_eq!(
        event_ids(&mut writer).await.len(),
        2,
        "the instruction's event and the fixture"
    );
    worker.stop().await;
}

#[tokio::test]
async fn a_terminal_push_goes_at_its_strict_cutoff_and_a_pending_one_holds_no_timer() {
    use collaboration_protocol::{
        EndpointId, EndpointRef, PushHeaderFacts, PushId, PushKind, PushOrigin, PushRecordDraft,
        SessionId, SessionRef, UuidIdentity,
    };
    let session = |endpoint: &str, id: &str| SessionRef {
        endpoint: EndpointRef {
            service_id: UuidIdentity::try_from("018f47d2-24d5-7a68-b9ec-6f759c39458f".to_owned())
                .unwrap(),
            endpoint_id: EndpointId::try_from(endpoint.to_owned()).unwrap(),
        },
        session_id: SessionId::try_from(id.to_owned()).unwrap(),
    };
    let draft = |created_at: DateTime<Utc>| PushRecordDraft {
        mode: Some(collaboration_protocol::MessageDelivery::Auto),
        guard: None,
        push_id: PushId::try_from(uuid::Uuid::now_v7().to_string()).unwrap(),
        kind: PushKind::DirectMessage,
        origin: PushOrigin::Session(session("claude-local", "sender")),
        origin_router_ref: None,
        target: session("codex-local", "target"),
        reply_to_push_id: None,
        header_facts: PushHeaderFacts::DirectMessage {
            sender_display_name: None,
        },
        body: Some("aging push".to_owned()),
        activity: None,
        created_at,
    };
    let directory = tempfile::tempdir().unwrap();
    let store = open_store(directory.path()).await;
    let interaction_path = directory.path().join("interaction.sqlite");
    let interactions = Arc::new(
        interaction_storage::InteractionStore::open(&interaction_path)
            .await
            .unwrap(),
    );
    // The older push is still pending, so it is protected; the newer one is delivered.
    let pending = draft(at("2026-01-10T00:00:00.000Z"));
    let delivered = draft(at("2026-01-15T10:00:05.000Z"));
    let delivered_id = delivered.push_id.clone();
    interactions.insert_push_record(pending).await.unwrap();
    interactions.insert_push_record(delivered).await.unwrap();
    let mut interaction_writer = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new().filename(&interaction_path),
    )
    .await
    .unwrap();
    sqlx::query::<sqlx_turso::Turso>(
        "UPDATE router_pushes SET delivery_state='delivered' WHERE push_id=?",
    )
    .bind(delivered_id.as_str())
    .execute(&mut interaction_writer)
    .await
    .unwrap();

    let worker = RunningWorker::start_with(
        &store,
        Some(interactions.clone()),
        at("2026-03-15T10:00:00.000Z"),
    )
    .await;

    let parked = worker.snapshot();
    assert_eq!(
        armed(&parked),
        Some(at("2026-03-15T10:00:05.001Z")),
        "the delivered push arms the timer; the older pending push does not"
    );
    assert!(prunes(&parked).is_empty());
    run_to_the_armed_time(&worker).await;
    let woke = worker.snapshot();
    assert_eq!(prunes(&woke), vec!["retention.interaction_records.prune"]);
    assert!(
        interactions
            .get_push_record(&delivered_id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(armed(&woke), None, "the pending push holds no timer");
    worker.stop().await;
    interaction_writer.close().await.unwrap();
}

#[tokio::test]
async fn settling_an_old_push_wakes_the_worker_through_the_interaction_store_and_it_goes() {
    use collaboration_protocol::{DeliveryOutcome, DeliveryReceipt};
    let directory = tempfile::tempdir().unwrap();
    let store = open_store(directory.path()).await;
    let interactions = Arc::new(
        interaction_storage::InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await
            .unwrap(),
    );
    let wall = at("2026-06-15T12:00:00.000Z");
    // Long past retention, but attempted and not yet settled, so protected. The attempt is
    // written before the worker starts, so settling is the only commit that follows.
    let push = aging_push(at("2026-01-10T00:00:00.000Z"));
    let push_id = push.push_id.clone();
    interactions.insert_push_record(push).await.unwrap();
    interactions.mark_push_attempted(&push_id).await.unwrap();
    let worker = RunningWorker::start_with(&store, Some(interactions.clone()), wall).await;
    let parked = worker.snapshot();
    assert_eq!(armed(&parked), None, "an unsettled push holds no timer");
    assert!(prunes(&parked).is_empty());

    // Settling it is an interaction-store commit; nothing touches the automation store.
    let before_settlement = worker.snapshot().sleeps;
    interactions
        .settle_push_record(
            &push_id,
            DeliveryReceipt {
                outcome: DeliveryOutcome::PeerMessageWritten,
                reachability: None,
                client: None,
            },
            wall,
        )
        .await
        .unwrap();
    (worker.observation.asleep_at_least(before_settlement + 1)).await;

    let woke = worker.snapshot();
    assert_eq!(
        woke.reads[parked.reads.len()..]
            .iter()
            .copied()
            .filter(|read| read.ends_with(".prune"))
            .collect::<Vec<_>>(),
        vec!["retention.interaction_records.prune"],
        "the push went at its due time, which had already passed"
    );
    assert!(
        interactions
            .get_push_record(&push_id)
            .await
            .unwrap()
            .is_none()
    );
    worker.stop().await;
}

#[tokio::test]
async fn settling_a_pending_approval_arms_the_history_timer_at_its_due_time() {
    use super::interaction_history_retention_tests::{load_broker, session};
    let directory = tempfile::tempdir().unwrap();
    let store = open_store(directory.path()).await;
    let gate = crate::NativeGenerationGate::default();
    let broker = load_broker(directory.path(), &gate).await.unwrap();
    let request: session_event_model::ApprovalRequest = serde_json::from_value(serde_json::json!({
        "requestId": "aging-approval", "title": "Run command", "options": [
            {"optionId": "allow-once", "label": "Allow once", "choice": {"effect": "allow", "scope": "once"}}
        ]
    }))
    .unwrap();
    let requester =
        serde_json::from_value(serde_json::to_value(session("requester").unwrap()).unwrap())
            .unwrap();
    let approver =
        serde_json::from_value(serde_json::json!({"kind": "human", "humanId": "retention-owner"}))
            .unwrap();
    let _resolution = broker
        .request_typed_approval(
            requester,
            approver,
            request,
            CancellationToken::new(),
            CancellationToken::new(),
            None,
        )
        .await
        .unwrap();
    let mut history = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(directory.path().join("interaction.sqlite"))
            .read_only(true),
    )
    .await
    .unwrap();
    let created_at: String = sqlx::query_scalar::<sqlx_turso::Turso, String>(
        "SELECT created_at FROM typed_interaction_history WHERE request_id='aging-approval'",
    )
    .fetch_one(&mut history)
    .await
    .unwrap();
    history.close().await.unwrap();
    let created_at = at(&created_at);
    let worker = RunningWorker::start_full(&store, None, Some(broker.clone()), created_at).await;
    let parked = worker.snapshot();
    assert_eq!(armed(&parked), None, "a pending approval holds no timer");

    // Cancelling settles the approval through the broker: an interaction-store commit.
    broker
        .cancel_typed_approval("aging-approval", "retention proof")
        .await
        .unwrap();
    (worker.observation.asleep_at_least(parked.sleeps + 1)).await;

    let expected_due =
        sqlite_storage_maintenance::first_deletable_at(created_at, 2, created_at).unwrap();
    let woke = worker.snapshot();
    assert_eq!(
        armed(&woke).map(|due| due.timestamp_millis()),
        Some(expected_due.timestamp_millis()),
        "the settled record arms the timer at the first moment its rule allows"
    );
    run_to_the_armed_time(&worker).await;
    assert_eq!(
        prunes(&worker.snapshot()),
        vec!["retention.interaction_history.prune"]
    );
    assert!(
        broker
            .list_interactions()
            .await
            .iter()
            .all(|record| record.request_id() != "aging-approval")
    );
    worker.stop().await;
}
