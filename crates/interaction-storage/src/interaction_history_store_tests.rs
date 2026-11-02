use super::super::{InteractionHistoryRecord, RefusedApprovalOption, RefusedTypedApproval};
use super::InteractionHistoryStore;
use chrono::{Duration, Utc};
use message_board::{Identity, SessionRef};
use sqlx::Connection;

#[path = "interaction_history_legacy_tests.rs"]
mod legacy_scenarios;
#[path = "interaction_history_sqlite_tests.rs"]
mod sqlite_scenarios;

fn session_ref(session_id: &str) -> SessionRef {
    serde_json::from_value(serde_json::json!({
        "endpoint":{
            "serviceId":"018f47d2-24d5-7a68-b9ec-6f759c39458f",
            "endpointId":"codex-local"
        },
        "sessionId":session_id
    }))
    .expect("valid fixture session")
}

fn refused_approval(request_id: &str) -> InteractionHistoryRecord {
    let requester = session_ref("requester");
    InteractionHistoryRecord::RefusedApproval {
        requester: requester.clone(),
        approver: Identity::Session { session: requester },
        refusal: RefusedTypedApproval {
            request_id: request_id.to_owned(),
            title: "Rejected approval".to_owned(),
            description: None,
            subject: None,
            options: vec![RefusedApprovalOption {
                option_id: "reject".to_owned(),
                label: "Reject".to_owned(),
                provider_kind: "fixture".to_owned(),
            }],
            reason: "fixture refusal".to_owned(),
        },
    }
}

#[tokio::test]
async fn sqlite_creation_reopens_records_without_touching_old_json() {
    let directory = tempfile::tempdir().expect("isolated SQLite fixture");
    let source = directory.path().join("interaction-history.json");
    let original = b"malformed obsolete JSON";
    tokio::fs::write(&source, original)
        .await
        .expect("obsolete file");
    let database = directory.path().join("interaction.sqlite");
    let store = InteractionHistoryStore::load(database.clone())
        .await
        .expect("fresh store");
    assert!(store.list_all().await.is_empty());
    sqlite_scenarios::add_refusal(&store, "new-refusal")
        .await
        .expect("real mutation");
    let first_stamp = store.data.lock().await.created_at["new-refusal"];
    drop(store);
    tokio::fs::write(&source, b"changed obsolete file")
        .await
        .expect("old writer");
    let reopened = InteractionHistoryStore::load(database)
        .await
        .expect("reopen");
    assert!(reopened.interaction("new-refusal").await.is_some());
    assert_eq!(
        reopened.data.lock().await.created_at["new-refusal"],
        first_stamp
    );
    assert_eq!(
        tokio::fs::read(source).await.expect("untouched old file"),
        b"changed obsolete file"
    );
    let mut observer = sqlite_scenarios::observer(directory.path()).await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM typed_interaction_history")
        .fetch_one(&mut observer)
        .await
        .expect("durable rows");
    assert_eq!(count, 1);
}

#[tokio::test]
async fn unrelated_database_is_rejected_without_adopting_its_schema() {
    let directory = tempfile::tempdir().expect("isolated foreign database");
    let database = directory.path().join("interaction.sqlite");
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&database)
        .create_if_missing(true);
    let mut foreign = <sqlx::SqliteConnection as sqlx::Connection>::connect_with(&options)
        .await
        .expect("foreign connection");
    sqlx::query("CREATE TABLE foreign_records (body TEXT NOT NULL)")
        .execute(&mut foreign)
        .await
        .expect("foreign schema");
    sqlx::query("INSERT INTO foreign_records VALUES ('keep this record')")
        .execute(&mut foreign)
        .await
        .expect("foreign record");
    sqlx::Connection::close(foreign)
        .await
        .expect("close foreign database");
    let original = tokio::fs::read(&database).await.expect("foreign bytes");
    assert!(
        InteractionHistoryStore::load(directory.path().join("interaction.sqlite"))
            .await
            .is_err(),
        "an unknown database must never be adopted"
    );
    assert_eq!(
        tokio::fs::read(database)
            .await
            .expect("foreign bytes retained"),
        original
    );
}

#[tokio::test]
async fn settled_history_prunes_after_two_calendar_months_but_pending_history_survives() {
    let directory = tempfile::tempdir().expect("isolated retention fixture");
    let path = directory.path().join("interaction.sqlite");
    let store = InteractionHistoryStore::load(path.clone())
        .await
        .expect("fresh store");
    let pending_request: session_event_model::ApprovalRequest =
        serde_json::from_value(serde_json::json!({
            "requestId":"pending-request","title":"Run command","options":[
                {"optionId":"allow-once","label":"Allow once","choice":{"effect":"allow","scope":"once"}}
            ]
        }))
        .expect("typed request");
    let pending = InteractionHistoryRecord::Approval {
        requester: session_ref("retention-requester"),
        approver: serde_json::from_value(serde_json::json!({
            "kind":"human","humanId":"retention-owner"
        }))
        .expect("human approver"),
        request: pending_request,
        state: super::super::InteractionHistoryState::Pending,
        legacy_metadata: None,
    };
    store.record(pending).await.expect("pending history");
    sqlite_scenarios::add_refusal(&store, "settled-expired")
        .await
        .expect("expired refusal");
    sqlite_scenarios::add_refusal(&store, "settled-boundary")
        .await
        .expect("boundary refusal");
    drop(store);

    let now: chrono::DateTime<Utc> = "2026-10-31T12:00:00Z".parse().unwrap();
    let cutoff = "2026-08-31T12:00:00Z"
        .parse::<chrono::DateTime<Utc>>()
        .unwrap();
    let expired = cutoff - chrono::Duration::nanoseconds(1);
    let mut observer = sqlite_scenarios::native_observer(directory.path()).await;
    for (request_id, created_at) in [
        ("pending-request", expired),
        ("settled-expired", expired),
        ("settled-boundary", cutoff),
    ] {
        sqlx::query("UPDATE typed_interaction_history SET created_at=? WHERE request_id=?")
            .bind(created_at.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true))
            .bind(request_id)
            .execute(&mut observer)
            .await
            .expect("fixed creation timestamp");
    }
    observer.close().await.expect("close fixture observer");

    let store = InteractionHistoryStore::load(path).await.expect("reopen");
    assert_eq!(
        store
            .prune_expired(now, 500)
            .await
            .expect("two-month pruning"),
        1,
        "only an expired settled record should be removed"
    );
    assert!(store.interaction("settled-expired").await.is_none());
    assert!(store.interaction("settled-boundary").await.is_some());
    assert!(store.interaction("pending-request").await.is_some());
    assert_eq!(
        store.data.lock().await.created_at["pending-request"],
        expired,
        "reload must preserve the pending record creation timestamp"
    );
    assert_eq!(
        store
            .prune_expired(now, 500)
            .await
            .expect("repeat maintenance"),
        0
    );
    let mut observer = sqlite_scenarios::observer(directory.path()).await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM typed_interaction_history")
        .fetch_one(&mut observer)
        .await
        .expect("durable prune");
    assert_eq!(count, 2);
}

#[tokio::test]
async fn native_terminal_history_reopens_validates_and_expires_by_original_creation() {
    use collaboration_protocol::{ApprovalDecision, ApprovalRequestRecord, ApprovalState};
    let directory = tempfile::tempdir().expect("isolated native history");
    let database = directory.path().join("interaction.sqlite");
    let store = InteractionHistoryStore::load(database.clone())
        .await
        .expect("history owner");
    let mut record: ApprovalRequestRecord = serde_json::from_value(serde_json::json!({
        "requestId":"native-history", "requester":session_ref("requester"),
        "approver":session_ref("approver"),
        "generation":{"serviceEpoch":"018f47d2-24d5-7a68-b9ec-6f759c39458f","generation":1},
        "state":"pendingClientDecision", "offeredOptions":[{"optionId":"native-accept","scope":{"kind":"allowOnce"}}],
        "operation":{"method":"approval"}, "expiresAt":Utc::now().to_rfc3339()
    })).expect("native facts");
    store
        .record_native_approval(record.clone())
        .await
        .expect("pending commit");
    let original_creation = store.data.lock().await.created_at["native-history"];
    record.state = ApprovalState::Decided;
    record.decision = Some(ApprovalDecision::Allow);
    store
        .record_native_approval(record.clone())
        .await
        .expect("decision commit");
    assert_eq!(
        store.data.lock().await.created_at["native-history"],
        original_creation
    );
    drop(store);
    let reopened = InteractionHistoryStore::load(database)
        .await
        .expect("reopen native decision");
    assert_eq!(
        reopened.interaction("native-history").await,
        Some(InteractionHistoryRecord::NativeApproval {
            record: Box::new(record.clone())
        })
    );
    let future = original_creation
        .checked_add_months(chrono::Months::new(3))
        .expect("retention clock");
    assert_eq!(
        reopened
            .prune_expired(future, 10)
            .await
            .expect("bounded prune"),
        1
    );
    record.decision = None;
    assert!(
        reopened.record_native_approval(record).await.is_err(),
        "decided rows require a decision"
    );
}

#[tokio::test]
async fn native_pending_history_is_cancelled_durably_on_restart() {
    let directory = tempfile::tempdir().expect("isolated native restart");
    let database = directory.path().join("interaction.sqlite");
    let store = InteractionHistoryStore::load(database.clone())
        .await
        .expect("history owner");
    let record = serde_json::from_value(serde_json::json!({
        "requestId":"native-pending", "requester":session_ref("requester"),
        "approver":session_ref("approver"),
        "generation":{"serviceEpoch":"018f47d2-24d5-7a68-b9ec-6f759c39458f","generation":1},
        "state":"pendingClientDecision", "operation":{}, "expiresAt":Utc::now().to_rfc3339()
    }))
    .expect("pending native facts");
    store
        .record_native_approval(record)
        .await
        .expect("pending commit");
    drop(store);
    let reopened = InteractionHistoryStore::load(database.clone())
        .await
        .expect("reopen");
    reopened
        .reconcile_pending_on_startup()
        .await
        .expect("reconcile pending");
    drop(reopened);
    let durable = InteractionHistoryStore::load(database)
        .await
        .expect("reopen cancellation");
    assert!(matches!(durable.interaction("native-pending").await,
        Some(InteractionHistoryRecord::NativeApproval { record }) if record.state == collaboration_protocol::ApprovalState::Cancelled && record.reason.as_deref() == Some("hostRestarted")));
}
