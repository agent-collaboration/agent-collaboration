//! Existing push repository invariants exercised through the native interaction owner.

use agent_automation::{OccurrenceId, RunId, ScheduleId, WakeupId};
use chrono::{Duration, Utc};
use collaboration_protocol::{
    EndpointId, EndpointRef, PushHeaderFacts, PushId, PushKind, PushOrigin, RouterOriginRef,
    SessionId, SessionRef, UuidIdentity,
};
use interaction_storage::{InteractionStore, PushRecordDraft, StorageError};
use sqlx::Connection;
use sqlx_turso::{TursoConnectOptions, TursoConnection};
use std::path::PathBuf;

struct TestDatabase {
    path: PathBuf,
}

impl TestDatabase {
    fn new() -> Self {
        Self {
            path: std::env::temp_dir().join(format!(
                "interaction-push-records-{}.sqlite",
                uuid::Uuid::now_v7()
            )),
        }
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[expect(
    clippy::expect_used,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]
fn session(endpoint_id: &str, session_id: &str) -> SessionRef {
    SessionRef {
        endpoint: EndpointRef {
            service_id: UuidIdentity::try_from("018f47d2-24d5-7a68-b9ec-6f759c39458f".to_owned())
                .expect("service UUID"),
            endpoint_id: EndpointId::try_from(endpoint_id.to_owned()).expect("endpoint id"),
        },
        session_id: SessionId::try_from(session_id.to_owned()).expect("session id"),
    }
}

fn push_draft(
    push_id: PushId,
    target: SessionRef,
    origin: PushOrigin,
    body: &str,
    created_at: chrono::DateTime<Utc>,
) -> PushRecordDraft {
    PushRecordDraft {
        mode: Some(collaboration_protocol::MessageDelivery::Auto),
        guard: None,
        push_id,
        kind: PushKind::DirectMessage,
        origin,
        origin_router_ref: None,
        target,
        reply_to_push_id: None,
        header_facts: PushHeaderFacts::DirectMessage {
            sender_display_name: None,
        },
        body: Some(body.to_owned()),
        activity: None,
        created_at,
    }
}

#[expect(
    clippy::expect_used,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]
fn push_id() -> PushId {
    PushId::try_from(uuid::Uuid::now_v7().to_string()).expect("UUIDv7 push id")
}

#[expect(
    clippy::expect_used,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]
fn wake_push_draft(
    push_id: PushId,
    target: SessionRef,
    wakeup_id: WakeupId,
    occurrence_id: OccurrenceId,
) -> PushRecordDraft {
    let origin_router_ref = RouterOriginRef::Wake {
        wakeup_id,
        occurrence_id,
    }
    .canonical_string()
    .expect("canonical wake origin reference");
    PushRecordDraft {
        mode: None,
        guard: None,
        push_id,
        kind: PushKind::Wake,
        origin: PushOrigin::Router(PushKind::Wake),
        origin_router_ref: Some(origin_router_ref),
        target,
        reply_to_push_id: None,
        header_facts: PushHeaderFacts::Wake,
        body: Some("A durable wake message".to_owned()),
        activity: None,
        created_at: Utc::now(),
    }
}

#[expect(
    clippy::expect_used,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]
fn schedule_run_push_draft(
    push_id: PushId,
    target: SessionRef,
    schedule_id: ScheduleId,
    run_id: RunId,
) -> PushRecordDraft {
    let origin_router_ref = RouterOriginRef::ScheduleRun {
        schedule_id: schedule_id.clone(),
        run_id: run_id.clone(),
    }
    .canonical_string()
    .expect("canonical schedule-run origin reference");
    PushRecordDraft {
        mode: None,
        guard: None,
        push_id,
        kind: PushKind::ScheduleRun,
        origin: PushOrigin::Router(PushKind::ScheduleRun),
        origin_router_ref: Some(origin_router_ref),
        target,
        reply_to_push_id: None,
        header_facts: PushHeaderFacts::ScheduleRun {
            schedule_id,
            run_id,
        },
        body: Some("A scheduled run".to_owned()),
        activity: None,
        created_at: Utc::now(),
    }
}

#[tokio::test]
async fn one_wake_occurrence_cannot_create_two_push_records() {
    let database = TestDatabase::new();
    let store = InteractionStore::open(&database.path)
        .await
        .expect("interaction store migrates");
    let target = session("codex-local", "wake-target");
    let wakeup_id = WakeupId::generate();
    let occurrence_id = OccurrenceId::generate();

    store
        .insert_push_record(wake_push_draft(
            push_id(),
            target.clone(),
            wakeup_id.clone(),
            occurrence_id.clone(),
        ))
        .await
        .expect("first push for wake occurrence");
    let duplicate = store
        .insert_push_record(wake_push_draft(push_id(), target, wakeup_id, occurrence_id))
        .await;

    assert!(matches!(duplicate, Err(StorageError::Database(_))));
    let mut observer = TursoConnection::connect_with(
        &TursoConnectOptions::new()
            .filename(&database.path)
            .foreign_keys(true),
    )
    .await
    .expect("open push observer");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM router_pushes WHERE kind='wake'")
        .fetch_one(&mut observer)
        .await
        .expect("read wake push count");
    assert_eq!(count, 1);
    observer.close().await.expect("close push observer");
}

#[tokio::test]
async fn wake_push_origin_reference_requires_canonical_wake_encoding() {
    let database = TestDatabase::new();
    let store = InteractionStore::open(&database.path)
        .await
        .expect("interaction store migrates");
    let target = session("codex-local", "wake-target");
    let wakeup_id = WakeupId::generate();
    let occurrence_id = OccurrenceId::generate();
    let draft = wake_push_draft(push_id(), target, wakeup_id.clone(), occurrence_id.clone());
    let mut noncanonical = draft.clone();
    noncanonical.origin_router_ref = Some(format!(
        "{} ",
        draft
            .origin_router_ref
            .as_deref()
            .expect("wake origin reference")
    ));

    assert!(matches!(
        store.insert_push_record(noncanonical).await,
        Err(StorageError::InvalidRecord)
    ));
    let inserted = store
        .insert_push_record(draft)
        .await
        .expect("canonical wake push");
    let canonical = inserted
        .origin_router_ref
        .as_deref()
        .expect("stored wake origin reference");
    let mut observer = TursoConnection::connect_with(
        &TursoConnectOptions::new()
            .filename(&database.path)
            .foreign_keys(true),
    )
    .await
    .expect("open push observer");
    sqlx::query("UPDATE router_pushes SET origin_router_ref=? WHERE push_id=?")
        .bind(format!("{canonical} "))
        .bind(inserted.push_id.as_str())
        .execute(&mut observer)
        .await
        .expect("corrupt stored wake origin reference");
    observer.close().await.expect("close push observer");

    assert!(matches!(
        store.get_push_record(&inserted.push_id).await,
        Err(StorageError::InvalidRecord)
    ));
}

#[tokio::test]
async fn schedule_run_origin_reference_requires_canonical_schedule_identity() {
    let database = TestDatabase::new();
    let store = InteractionStore::open(&database.path)
        .await
        .expect("interaction store migrates");
    let target = session("codex-local", "schedule-target");
    let schedule_id = ScheduleId::generate();
    let run_id = RunId::generate();
    let draft = schedule_run_push_draft(push_id(), target, schedule_id, run_id);

    let mut noncanonical = draft.clone();
    noncanonical.origin_router_ref = Some(format!(
        "{} ",
        draft
            .origin_router_ref
            .as_deref()
            .expect("schedule-run origin reference")
    ));
    assert!(matches!(
        store.insert_push_record(noncanonical).await,
        Err(StorageError::InvalidRecord)
    ));

    let mut wrong_kind = draft.clone();
    wrong_kind.origin_router_ref = Some(
        RouterOriginRef::Wake {
            wakeup_id: WakeupId::generate(),
            occurrence_id: OccurrenceId::generate(),
        }
        .canonical_string()
        .expect("canonical wrong-kind origin reference"),
    );
    assert!(matches!(
        store.insert_push_record(wrong_kind).await,
        Err(StorageError::InvalidRecord)
    ));

    let inserted = store
        .insert_push_record(draft)
        .await
        .expect("canonical schedule-run push");
    let canonical = inserted
        .origin_router_ref
        .as_deref()
        .expect("stored schedule-run origin reference")
        .to_owned();
    let decoded = store
        .get_push_record(&inserted.push_id)
        .await
        .expect("read schedule-run push")
        .expect("schedule-run push exists");
    assert_eq!(
        decoded.origin_router_ref.as_deref(),
        Some(canonical.as_str())
    );

    let wrong_kind = RouterOriginRef::Wake {
        wakeup_id: WakeupId::generate(),
        occurrence_id: OccurrenceId::generate(),
    }
    .canonical_string()
    .expect("canonical wrong-kind stored origin reference");
    let mut observer = TursoConnection::connect_with(
        &TursoConnectOptions::new()
            .filename(&database.path)
            .foreign_keys(true),
    )
    .await
    .expect("open push observer");
    sqlx::query("UPDATE router_pushes SET origin_router_ref=? WHERE push_id=?")
        .bind(wrong_kind)
        .bind(inserted.push_id.as_str())
        .execute(&mut observer)
        .await
        .expect("corrupt stored schedule-run origin reference");
    observer.close().await.expect("close push observer");

    assert!(matches!(
        store.get_push_record(&inserted.push_id).await,
        Err(StorageError::InvalidRecord)
    ));
}

#[tokio::test]
async fn header_facts_round_trip_through_stored_json_uses_camel_case() {
    let database = TestDatabase::new();
    let store = InteractionStore::open(&database.path)
        .await
        .expect("interaction store migrates");
    let header_facts = PushHeaderFacts::DirectMessage {
        sender_display_name: None,
    };
    let push_id = push_id();
    let mut draft = push_draft(
        push_id.clone(),
        session("codex-local", "target-thread"),
        PushOrigin::Session(session("claude-local", "sender-thread")),
        "stored body",
        Utc::now(),
    );
    draft.header_facts = header_facts.clone();
    store
        .insert_push_record(draft)
        .await
        .expect("store push header facts");

    let mut observer = TursoConnection::connect_with(
        &TursoConnectOptions::new()
            .filename(&database.path)
            .foreign_keys(true),
    )
    .await
    .expect("open isolated JSON observer");
    let stored_json: String =
        sqlx::query_scalar("SELECT header_facts_json FROM router_pushes WHERE push_id=?")
            .bind(push_id.as_str())
            .fetch_one(&mut observer)
            .await
            .expect("read serialized header facts");
    let stored_value: serde_json::Value =
        serde_json::from_str(&stored_json).expect("stored header facts JSON");
    assert_eq!(stored_value["kind"], "directMessage");
    assert_eq!(stored_value["senderDisplayName"], serde_json::Value::Null);
    assert!(stored_value.get("sender_display_name").is_none());

    let decoded = store
        .get_push_record(&push_id)
        .await
        .expect("decode stored push")
        .expect("stored push remains available");
    assert_eq!(decoded.header_facts, header_facts);
    observer
        .close()
        .await
        .expect("close isolated JSON observer");
    store.close().await.expect("close interaction store");
}

#[tokio::test]
async fn prune_removes_only_push_records_strictly_older_than_thirty_days() {
    let database = TestDatabase::new();
    let store = InteractionStore::open(&database.path)
        .await
        .expect("interaction store migrates");
    let sender = session("claude-local", "sender-thread");
    let target = session("codex-local", "target-thread");
    let now = Utc::now();
    let cutoff = now - Duration::days(30);
    let expired_id = push_id();
    let boundary_id = push_id();
    let recent_id = push_id();
    for (id, timestamp, body) in [
        (
            expired_id.clone(),
            cutoff - Duration::nanoseconds(1),
            "expired",
        ),
        (boundary_id.clone(), cutoff, "boundary"),
        (recent_id.clone(), now - Duration::seconds(1), "recent"),
    ] {
        store
            .insert_push_record(push_draft(
                id,
                target.clone(),
                PushOrigin::Session(sender.clone()),
                body,
                timestamp,
            ))
            .await
            .expect("insert retention fixture");
    }

    assert_eq!(
        store
            .prune_push_records(now, 500)
            .await
            .expect("prune one bounded batch"),
        1
    );
    assert!(
        store
            .get_push_record(&expired_id)
            .await
            .expect("read expired")
            .is_none()
    );
    assert!(
        store
            .get_push_record(&boundary_id)
            .await
            .expect("read boundary")
            .is_some()
    );
    assert!(
        store
            .get_push_record(&recent_id)
            .await
            .expect("read recent")
            .is_some()
    );
    store.close().await.expect("close interaction store");
}

#[tokio::test]
async fn direct_message_over_sixty_four_kib_is_rejected_before_persistence() {
    let database = TestDatabase::new();
    let store = InteractionStore::open(&database.path)
        .await
        .expect("interaction store migrates");
    let sender = session("claude-local", "sender-thread");
    let target = session("codex-local", "target-thread");
    let push_id = push_id();
    let draft = push_draft(
        push_id.clone(),
        target,
        PushOrigin::Session(sender),
        &"x".repeat(65_537),
        Utc::now(),
    );

    assert!(matches!(
        store.insert_push_record(draft).await,
        Err(StorageError::InvalidRecord)
    ));
    assert!(
        store
            .get_push_record(&push_id)
            .await
            .expect("check absent record")
            .is_none()
    );
    store.close().await.expect("close interaction store");
}

#[tokio::test]
async fn insert_retries_native_busy_only_within_the_five_second_window() {
    let database = TestDatabase::new();
    let store = InteractionStore::open(&database.path)
        .await
        .expect("interaction store migrates");
    let mut writer = TursoConnection::connect_with(
        &TursoConnectOptions::new()
            .filename(&database.path)
            .create_if_missing(true)
            .foreign_keys(true),
    )
    .await
    .expect("open independent lock holder");
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut writer)
        .await
        .expect("hold Turso writer lock");
    let sender = session("claude-local", "sender-thread");
    let target = session("codex-local", "target-thread");
    let push_id = push_id();
    let (subscriber, busy) = controlled_busy::observe_push(push_id.as_str());
    let _observation = tracing::subscriber::set_default(subscriber);
    let draft = push_draft(
        push_id.clone(),
        target,
        PushOrigin::Session(sender),
        "must not be submitted while persistence is busy",
        Utc::now(),
    );

    let result =
        controlled_busy::complete_after_actual_busy(busy, store.insert_push_record(draft)).await;
    sqlx::query("ROLLBACK")
        .execute(&mut writer)
        .await
        .expect("release Turso writer lock");
    writer.close().await.expect("close lock holder");
    let result = result.expect("busy retries finish within the specified bound");
    assert!(matches!(result, Err(StorageError::Database(_))));
    assert!(
        store
            .get_push_record(&push_id)
            .await
            .expect("read after busy")
            .is_none()
    );
    store.close().await.expect("close interaction store");
}

#[path = "push_repository/controlled_busy.rs"]
mod controlled_busy;

#[path = "push_repository/reply_references.rs"]
mod reply_references;

#[path = "push_repository/carrier_outcomes.rs"]
mod carrier_outcomes;
