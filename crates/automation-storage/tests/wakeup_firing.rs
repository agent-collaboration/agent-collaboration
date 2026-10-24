use agent_automation::{
    DurableMessage, ExpiryRule, FirstFire, OperationId, TimingRule, WakeRecord,
};
use automation_storage::{AutomationStore, StorageError, WakeCreate, WakeEvaluation};
use collaboration_protocol::PushRecordDraft;
use collaboration_protocol::{
    MessageContent, PushHeaderFacts, PushId, PushKind, PushOrigin, RouterOriginRef, SavedMessage,
    SessionRef,
};
use serde::{Deserialize, Serialize};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::path::PathBuf;
#[path = "support/wake_push_draft.rs"]
mod wake_push_test_support;
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Message {
    target: String,
    text: String,
}
impl DurableMessage for Message {
    type Target = String;
    type Content = String;
    type Generation = String;
    fn target(&self) -> &String {
        &self.target
    }
    fn content(&self) -> &String {
        &self.text
    }
    fn generation_guard(&self) -> Option<&String> {
        None
    }
    fn delivery_mode(&self) -> &'static str {
        "auto"
    }
}

fn saved_message() -> Result<SavedMessage, serde_json::Error> {
    serde_json::from_value(serde_json::json!({
        "target": {
            "endpoint": {
                "serviceId": "00000000-0000-4000-8000-000000000001",
                "endpointId": "codex-local"
            },
            "sessionId": "wake-target"
        },
        "content": {"kind":"humanUser", "text":"Check the durable job"},
        "delivery": "queue"
    }))
}

fn wake_push_draft(
    wake: &WakeRecord<SavedMessage, collaboration_protocol::WakeCreator>,
    fire: &FirstFire,
) -> Result<PushRecordDraft, StorageError> {
    if fire.wakeup_id != wake.definition.wakeup_id {
        return Err(StorageError::InvalidRecord);
    }
    let body = match &wake.definition.message.content {
        MessageContent::Agent { text, .. }
        | MessageContent::HumanUser { text }
        | MessageContent::Router { text } => text.as_str().to_owned(),
    };
    let origin_router_ref = RouterOriginRef::Wake {
        wakeup_id: fire.wakeup_id.clone(),
        occurrence_id: fire.occurrence_id.clone(),
    }
    .canonical_string()
    .map_err(|_| StorageError::InvalidRecord)?;
    let push_id = PushId::try_from(uuid::Uuid::now_v7().to_string())
        .map_err(|_| StorageError::InvalidRecord)?;
    let created_at = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(fire.fired_at_ms)
        .ok_or(StorageError::InvalidRecord)?;
    Ok(PushRecordDraft {
        mode: None,
        guard: None,
        push_id,
        kind: PushKind::Wake,
        origin: PushOrigin::Router(PushKind::Wake),
        origin_router_ref: Some(origin_router_ref),
        target: wake.definition.message.target.clone(),
        reply_to_push_id: None,
        header_facts: PushHeaderFacts::Wake,
        body: Some(body),
        activity: None,
        created_at,
    })
}

fn temporary_database_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "wake-firing-{}.sqlite",
        OperationId::generate().as_str()
    ))
}

fn router_origin_for_fire(fire: &FirstFire) -> RouterOriginRef {
    RouterOriginRef::Wake {
        wakeup_id: fire.wakeup_id.clone(),
        occurrence_id: fire.occurrence_id.clone(),
    }
}

async fn owed_push_for_origin(
    store: &mut AutomationStore,
    origin: &RouterOriginRef,
) -> Result<PushRecordDraft, Box<dyn std::error::Error>> {
    let expected = origin.canonical_string()?;
    store
        .list_owed_push_operations(100)
        .await?
        .into_iter()
        .filter_map(|row| match row.operation {
            automation_storage::PushOperation::Admit { push } => Some(*push),
            _ => None,
        })
        .find(|push| push.origin_router_ref.as_deref() == Some(expected.as_str()))
        .ok_or_else(|| "owed wake push missing".into())
}

// U66 / MS-10: valid bounded timing; original interleavings and assertions remain.
#[tokio::test]
async fn firing_persists_one_obligation_and_coalesces_until_delivery_resolves()
-> Result<(), Box<dyn std::error::Error>> {
    // Arrange: a repeating wake-up with no native receiver attached.
    let path = std::env::temp_dir().join(format!(
        "wake-firing-{}.sqlite",
        OperationId::generate().as_str()
    ));
    let mut store = AutomationStore::open(&path).await?;
    let wake = store
        .create_wakeup(&WakeCreate {
            requested_by: collaboration_protocol::WakeCreator::Owner {},
            replace: false,
            operation_id: OperationId::generate(),
            message: Message {
                target: "B".into(),
                text: "Check job".into(),
            },
            timing: TimingRule::Interval { seconds: 300 },
            expiry: ExpiryRule::After { seconds: 7200 },
            now_ms: 0,
        })
        .await?;
    // Act: overdue firing persists an obligation; later tick coalesces while unsent.
    let fired = store
        .evaluate_wakeup::<Message>(
            &wake.definition.wakeup_id,
            900000,
            wake_push_test_support::build_test_wake_push_draft,
        )
        .await?;
    let (first, delivery) = match fired {
        WakeEvaluation::Fired {
            fire, delivery_id, ..
        } => (fire, delivery_id),
        _ => return Err("wake did not fire".into()),
    };
    let coalesced = store
        .evaluate_wakeup::<Message>(
            &wake.definition.wakeup_id,
            1200000,
            wake_push_test_support::build_test_wake_push_draft,
        )
        .await?;
    if !matches!(coalesced,WakeEvaluation::Coalesced{delivery_id,..} if delivery_id==delivery) {
        return Err("unresolved wake did not coalesce".into());
    }
    store.close().await?;
    // Assert: reopening and removing old events cannot erase first firing or pending content.
    let mut check =
        SqliteConnection::connect_with(&sqlx::sqlite::SqliteConnectOptions::new().filename(&path))
            .await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mailbox_deliveries")
        .fetch_one(&mut check)
        .await?;
    if count != 1 {
        return Err("duplicate reminder obligations".into());
    }
    let receipt: Option<String> =
        sqlx::query_scalar("SELECT outcome_receipt_json FROM mailbox_deliveries")
            .fetch_one(&mut check)
            .await?;
    if receipt.is_some() {
        return Err("firing invented native acceptance".into());
    }
    check.close().await?;
    let mut store = AutomationStore::open(&path).await?;
    let maintenance_time =
        chrono::DateTime::parse_from_rfc3339("2026-08-31T12:00:00Z")?.timestamp_millis();
    if store
        .prune_automation_events(maintenance_time, 1000)
        .await?
        == 0
    {
        return Err("wake retention proof did not remove old events".into());
    }
    let current = store
        .read_wakeup::<Message>(&wake.definition.wakeup_id)
        .await?;
    if current.first_fire != Some(first) || current.pending_delivery_id != Some(delivery) {
        return Err("event cleanup erased current wake state".into());
    }
    store.close().await?;
    std::fs::remove_file(path)?;
    Ok(())
}

#[tokio::test]
async fn owed_wake_push_and_mailbox_commit_atomically_and_recover_after_restart()
-> Result<(), Box<dyn std::error::Error>> {
    let path = temporary_database_path();
    let mut store = AutomationStore::open(&path).await?;
    let wake = store
        .create_wakeup(&WakeCreate {
            requested_by: collaboration_protocol::WakeCreator::Owner {},
            replace: false,
            operation_id: OperationId::generate(),
            message: saved_message()?,
            timing: TimingRule::After { seconds: 1 },
            expiry: ExpiryRule::None,
            now_ms: 0,
        })
        .await?;
    let (fire, delivery_id) = match store
        .evaluate_wakeup::<SavedMessage>(&wake.definition.wakeup_id, 1_000, wake_push_draft)
        .await?
    {
        WakeEvaluation::Fired {
            fire, delivery_id, ..
        } => (fire, delivery_id),
        _ => return Err("wake did not fire".into()),
    };
    let origin = router_origin_for_fire(&fire);
    let committed_push = owed_push_for_origin(&mut store, &origin).await?;
    let committed_delivery = store
        .read_delivery::<SessionRef, collaboration_protocol::CodexGeneration, serde_json::Value>(
            &delivery_id,
        )
        .await?;
    if committed_delivery.occurrence_id != fire.occurrence_id {
        return Err("mailbox occurrence differs from committed push identity".into());
    }
    store.close().await?;

    let mut restarted = AutomationStore::open(&path).await?;
    let recovered_push = owed_push_for_origin(&mut restarted, &origin).await?;
    let eligible_delivery_ids = restarted.eligible_delivery_ids(1_000, 4).await?;
    if recovered_push.push_id != committed_push.push_id
        || recovered_push.body.as_deref() != Some("Check the durable job")
        || eligible_delivery_ids.as_slice() != std::slice::from_ref(&delivery_id)
    {
        return Err("restart did not recover the exact pending wake push".into());
    }

    restarted.close().await?;
    std::fs::remove_file(path)?;
    Ok(())
}

#[tokio::test]
async fn failed_owed_wake_insert_rolls_back_mailbox_and_firing_state()
-> Result<(), Box<dyn std::error::Error>> {
    let path = temporary_database_path();
    let mut store = AutomationStore::open(&path).await?;
    let wake = store
        .create_wakeup(&WakeCreate {
            requested_by: collaboration_protocol::WakeCreator::Owner {},
            replace: false,
            operation_id: OperationId::generate(),
            message: saved_message()?,
            timing: TimingRule::After { seconds: 1 },
            expiry: ExpiryRule::None,
            now_ms: 0,
        })
        .await?;
    let wakeup_id = wake.definition.wakeup_id;
    let mut trigger_connection =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    sqlx::query(
        "CREATE TRIGGER reject_wake_push BEFORE INSERT ON owed_push_operations BEGIN SELECT RAISE(ABORT, 'forced wake push insert failure'); END",
    )
    .execute(&mut trigger_connection)
    .await?;
    trigger_connection.close().await?;

    let result = store
        .evaluate_wakeup::<SavedMessage>(&wakeup_id, 1_000, wake_push_draft)
        .await;
    if !matches!(result, Err(StorageError::Database(_))) {
        return Err("forced push insert failure was not returned".into());
    }
    let current = store.read_wakeup::<SavedMessage>(&wakeup_id).await?;
    if current.pending_delivery_id.is_some()
        || current.first_fire.is_some()
        || current.next_due_at_ms != Some(1_000)
        || !store.due_wakeup_ids(1_000, 4).await?.contains(&wakeup_id)
    {
        return Err("failed push insert left firing state committed".into());
    }
    let mut observer =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    let mailbox_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM mailbox_deliveries WHERE wakeup_id=?")
            .bind(wakeup_id.as_str())
            .fetch_one(&mut observer)
            .await?;
    let fired_event_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM automation_events WHERE subject_id=? AND event_kind='fired'",
    )
    .bind(wakeup_id.as_str())
    .fetch_one(&mut observer)
    .await?;
    let owed_count: i64 = sqlx::query_scalar("SELECT count(*) FROM owed_push_operations")
        .fetch_one(&mut observer)
        .await?;
    observer.close().await?;
    if mailbox_count != 0 || fired_event_count != 0 || owed_count != 0 {
        return Err("failed push insert committed mailbox or fire event".into());
    }

    store.close().await?;
    std::fs::remove_file(path)?;
    Ok(())
}

// U66 / MS-10: valid bounded timing; original interleavings and assertions remain.
#[tokio::test]
async fn a_later_firing_of_the_same_wake_definition_gets_a_new_push()
-> Result<(), Box<dyn std::error::Error>> {
    let path = temporary_database_path();
    let mut store = AutomationStore::open(&path).await?;
    let wake = store
        .create_wakeup(&WakeCreate {
            requested_by: collaboration_protocol::WakeCreator::Owner {},
            replace: false,
            operation_id: OperationId::generate(),
            message: saved_message()?,
            timing: TimingRule::Interval { seconds: 300 },
            expiry: ExpiryRule::After { seconds: 7200 },
            now_ms: 0,
        })
        .await?;
    let (first_fire, first_delivery_id) = match store
        .evaluate_wakeup::<SavedMessage>(&wake.definition.wakeup_id, 900000, wake_push_draft)
        .await?
    {
        WakeEvaluation::Fired {
            fire, delivery_id, ..
        } => (fire, delivery_id),
        _ => return Err("first wake did not fire".into()),
    };
    let first_push = owed_push_for_origin(&mut store, &router_origin_for_fire(&first_fire)).await?;
    let mut observer =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    sqlx::query("UPDATE mailbox_deliveries SET delivery_status='accepted' WHERE delivery_id=?")
        .bind(first_delivery_id.as_str())
        .execute(&mut observer)
        .await?;
    observer.close().await?;

    let (second_fire, second_delivery_id) = match store
        .evaluate_wakeup::<SavedMessage>(&wake.definition.wakeup_id, 1200000, wake_push_draft)
        .await?
    {
        WakeEvaluation::Fired {
            fire, delivery_id, ..
        } => (fire, delivery_id),
        _ => return Err("later wake did not fire".into()),
    };
    let second_push =
        owed_push_for_origin(&mut store, &router_origin_for_fire(&second_fire)).await?;
    if first_fire.occurrence_id == second_fire.occurrence_id
        || first_push.push_id == second_push.push_id
        || first_delivery_id == second_delivery_id
    {
        return Err("later wake firing reused the earlier push identity".into());
    }
    let mut observer =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM owed_push_operations")
        .fetch_one(&mut observer)
        .await?;
    observer.close().await?;
    if count != 2 {
        return Err("wake definition did not retain one push per firing".into());
    }

    store.close().await?;
    std::fs::remove_file(path)?;
    Ok(())
}
