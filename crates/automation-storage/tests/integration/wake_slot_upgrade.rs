//! A real pre-release store is upgraded before the occupying-slot index is installed.
use agent_automation::{OperationId, WakeupId};
use automation_storage::AutomationStore;
use collaboration_protocol::{SavedMessage, WakeCreator, WakeRefusal};
use serde_json::{Value, json};
use sqlx::{Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};

macro_rules! ensure {
    ($condition:expr $(,)?) => { ensure!($condition, "assertion failed: {}", stringify!($condition)); };
    ($condition:expr, $($context:tt)+) => {
        if !$condition { return Err(format!($($context)+).into()); }
    };
}
macro_rules! ensure_eq {
    ($left:expr, $right:expr $(,)?) => { ensure_eq!($left, $right, "values differ"); };
    ($left:expr, $right:expr, $($context:tt)+) => {{
        let left = &$left; let right = &$right;
        if left != right { return Err(format!("{}: left={left:?}, right={right:?}", format_args!($($context)+)).into()); }
    }};
}
macro_rules! ensure_ne {
    ($left:expr, $right:expr $(,)?) => {
        ensure!($left != $right, "values must differ");
    };
}
type TestResult<TValue = ()> = Result<TValue, Box<dyn std::error::Error>>;
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");
const PRE_WAKE: i64 = 20_261_015_000_200;

async fn legacy_wake(
    connection: &mut SqliteConnection,
    target: &str,
    status: &str,
    created: i64,
    timing: Value,
    expiry: Option<i64>,
) -> TestResult<WakeupId> {
    let id = WakeupId::generate();
    let definition = json!({"wakeupId":id,"changeId":OperationId::generate(),"message":{"target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":target},"content":{"kind":"humanUser","text":"legacy wake"},"delivery":"auto"},"timing":timing,"anchorAtMs":created,"expiresAtMs":expiry,"createdAtMs":created});
    sqlx::query("INSERT INTO wakeup_definitions(wakeup_id,change_id,wakeup_status,definition_json,anchor_at_ms,evaluated_through_ms,next_due_at_ms,expires_at_ms,first_fire_json,pending_delivery_id,created_at_ms,updated_at_ms) VALUES (?,?,?,?,?,?,?,?,NULL,NULL,?,?)")
        .bind(id.as_str()).bind(definition.get("changeId").and_then(Value::as_str)).bind(status).bind(definition.to_string()).bind(created).bind(created).bind(created + 600_000).bind(expiry).bind(created).bind(created).execute(connection).await?;
    Ok(id)
}

#[tokio::test]
async fn upgrade_keeps_newest_expires_duplicates_caps_lifetimes_and_refuses_fast_repeats()
-> TestResult {
    let path = std::env::temp_dir().join(format!(
        "m2c-wake-upgrade-{}.sqlite",
        OperationId::generate().as_str()
    ));
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true),
    )
    .await?;
    MIGRATOR
        .run_direct(Some(PRE_WAKE), &mut connection, false)
        .await?;
    let now = chrono::Utc::now().timestamp_millis();
    let long_end = Some(now + 60 * 86_400_000);
    let older = legacy_wake(
        &mut connection,
        "duplicate",
        "active",
        now - 2000,
        json!({"kind":"interval","seconds":600}),
        long_end,
    )
    .await?;
    let newest = legacy_wake(
        &mut connection,
        "duplicate",
        "paused",
        now - 1000,
        json!({"kind":"interval","seconds":600}),
        long_end,
    )
    .await?;
    let fast = legacy_wake(
        &mut connection,
        "fast",
        "active",
        now - 1000,
        json!({"kind":"cron","expression":"* * * * *","timezone":"UTC"}),
        long_end,
    )
    .await?;
    let one_shot = legacy_wake(
        &mut connection,
        "far-ahead",
        "active",
        now - 1000,
        json!({"kind":"after","seconds":172800}),
        None,
    )
    .await?;
    connection.close().await?;
    let before = chrono::Utc::now().timestamp_millis();
    AutomationStore::open(&path).await?.close().await?;
    let after = chrono::Utc::now().timestamp_millis();
    let mut connection =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM wakeup_definitions WHERE wakeup_status IN ('active','paused') AND json_extract(definition_json,'$.message.target.sessionId')='duplicate'").fetch_one(&mut connection).await?;
    ensure_eq!(count, 1, "migration left duplicate occupying wakes");
    for (id, expected_status, reason) in [
        (older, "expired", Some("upgradeDuplicate")),
        (newest, "paused", None),
        (fast, "expired", Some("upgradeLimit")),
        (one_shot, "active", None),
    ] {
        let row = sqlx::query("SELECT definition_json,wakeup_status,expires_at_ms FROM wakeup_definitions WHERE wakeup_id=?").bind(id.as_str()).fetch_one(&mut connection).await?;
        ensure_eq!(row.try_get::<String, _>("wakeup_status")?, expected_status);
        let definition: Value =
            serde_json::from_str(&row.try_get::<String, _>("definition_json")?)?;
        ensure_eq!(definition["createdBy"], json!({"kind":"unrecorded"}));
        if let Some(reason) = reason {
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM automation_events WHERE subject_kind='wake' AND subject_id=? AND event_kind=?").bind(id.as_str()).bind(reason).fetch_one(&mut connection).await?;
            ensure_eq!(count, 1, "upgrade omitted its reason event");
        } else {
            let cap = if definition["timing"]["kind"] == "after" {
                86_400_000
            } else {
                604_800_000
            };
            let end: i64 = row.try_get("expires_at_ms")?;
            ensure!((before + cap..=after + cap).contains(&end));
            ensure_eq!(definition["expiresAtMs"], end);
        }
    }
    connection.close().await?;
    AutomationStore::open(&path).await?.close().await?;
    std::fs::remove_file(path)?;
    Ok(())
}

#[tokio::test]
async fn upgraded_unrecorded_creator_is_replaceable_only_by_target_or_owner() -> TestResult {
    for by_owner in [false, true] {
        let path = std::env::temp_dir().join(format!(
            "m2c-unrecorded-{}.sqlite",
            OperationId::generate().as_str()
        ));
        let mut connection = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await?;
        MIGRATOR
            .run_direct(Some(PRE_WAKE), &mut connection, false)
            .await?;
        let now = chrono::Utc::now().timestamp_millis();
        let id = legacy_wake(
            &mut connection,
            "unrecorded-target",
            "paused",
            now,
            json!({"kind":"after","seconds":600}),
            None,
        )
        .await?;
        connection.close().await?;
        let mut store = AutomationStore::open(&path).await?;
        let legacy = store.read_wakeup::<SavedMessage>(&id).await?;
        ensure_eq!(legacy.definition.created_by, WakeCreator::Unrecorded {});
        let target = legacy.definition.message.target.clone();
        let mut outsider = target.clone();
        outsider.session_id = "unrelated-session".to_owned().try_into()?;
        let mut input = automation_storage::WakeCreate {
            operation_id: OperationId::generate(),
            requested_by: WakeCreator::Session(outsider),
            replace: true,
            message: legacy.definition.message,
            timing: agent_automation::TimingRule::After { seconds: 600 },
            expiry: agent_automation::ExpiryRule::None,
            now_ms: now + 1,
        };
        ensure!(
            matches!(store.create_wakeup(&input).await, Err(automation_storage::StorageError::WakeRefused(WakeRefusal::WakeExists { wakeup, creator: WakeCreator::Unrecorded {} })) if wakeup == id)
        );
        ensure_eq!(
            store.read_wakeup::<SavedMessage>(&id).await?.state,
            agent_automation::WakeState::Paused
        );
        input.operation_id = OperationId::generate();
        input.requested_by = if by_owner {
            WakeCreator::Owner {}
        } else {
            WakeCreator::Session(target)
        };
        let replacement = store.create_wakeup(&input).await?;
        ensure_ne!(replacement.definition.wakeup_id, id);
        ensure_eq!(
            store.read_wakeup::<SavedMessage>(&id).await?.state,
            agent_automation::WakeState::Cancelled
        );
        store.close().await?;
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[tokio::test]
async fn occupying_target_index_refuses_a_second_row_and_upgrade_does_not_repeat() -> TestResult {
    let path = std::env::temp_dir().join(format!(
        "m2c-slot-index-{}.sqlite",
        OperationId::generate().as_str()
    ));
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true),
    )
    .await?;
    MIGRATOR
        .run_direct(Some(PRE_WAKE), &mut connection, false)
        .await?;
    let now = chrono::Utc::now().timestamp_millis();
    let id = legacy_wake(
        &mut connection,
        "slot-index-target",
        "active",
        now,
        json!({"kind":"after","seconds":600}),
        None,
    )
    .await?;
    connection.close().await?;
    AutomationStore::open(&path).await?.close().await?;
    let mut connection =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    let before: String =
        sqlx::query_scalar("SELECT definition_json FROM wakeup_definitions WHERE wakeup_id=?")
            .bind(id.as_str())
            .fetch_one(&mut connection)
            .await?;
    let mut duplicate: Value = serde_json::from_str(&before)?;
    let second = WakeupId::generate();
    duplicate
        .as_object_mut()
        .ok_or("duplicate object")?
        .insert("wakeupId".into(), json!(second));
    let result = sqlx::query("INSERT INTO wakeup_definitions(wakeup_id,change_id,wakeup_status,definition_json,anchor_at_ms,evaluated_through_ms,next_due_at_ms,expires_at_ms,first_fire_json,pending_delivery_id,created_at_ms,updated_at_ms) SELECT ?,change_id,'paused',?,anchor_at_ms,evaluated_through_ms,next_due_at_ms,expires_at_ms,NULL,NULL,created_at_ms,updated_at_ms FROM wakeup_definitions WHERE wakeup_id=?")
        .bind(second.as_str()).bind(duplicate.to_string()).bind(id.as_str()).execute(&mut connection).await;
    ensure!(
        matches!(&result, Err(error) if error.to_string().to_lowercase().contains("unique")),
        "a distinct ID entered the same occupying target slot: {result:?}"
    );
    let events: i64 = sqlx::query_scalar("SELECT count(*) FROM automation_events")
        .fetch_one(&mut connection)
        .await?;
    connection.close().await?;
    AutomationStore::open(&path).await?.close().await?;
    let mut connection =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    let after: String =
        sqlx::query_scalar("SELECT definition_json FROM wakeup_definitions WHERE wakeup_id=?")
            .bind(id.as_str())
            .fetch_one(&mut connection)
            .await?;
    let after_events: i64 = sqlx::query_scalar("SELECT count(*) FROM automation_events")
        .fetch_one(&mut connection)
        .await?;
    ensure_eq!(before, after);
    ensure_eq!(events, after_events);
    connection.close().await?;
    std::fs::remove_file(path)?;
    Ok(())
}

#[tokio::test]
async fn upgrade_preserves_readable_original_operation_answers_with_unknown_creator() -> TestResult
{
    let path = std::env::temp_dir().join(format!(
        "m2c-wake-receipt-{}.sqlite",
        OperationId::generate().as_str()
    ));
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true),
    )
    .await?;
    MIGRATOR
        .run_direct(Some(PRE_WAKE), &mut connection, false)
        .await?;
    let now = chrono::Utc::now().timestamp_millis();
    let id = legacy_wake(
        &mut connection,
        "receipt-target",
        "active",
        now,
        json!({"kind":"after","seconds":600}),
        None,
    )
    .await?;
    let definition: String =
        sqlx::query_scalar("SELECT definition_json FROM wakeup_definitions WHERE wakeup_id=?")
            .bind(id.as_str())
            .fetch_one(&mut connection)
            .await?;
    let definition: Value = serde_json::from_str(&definition)?;
    let original = json!({"definition":definition,"state":"active","nextDueAtMs":now+600_000,"firstFire":null,"pendingDeliveryId":null,"latestEventSequence":0});
    let canonical =
        serde_json::to_vec(&json!([definition["message"],definition["timing"],{"kind":"none"}]))?;
    let mut cases = Vec::new();
    for (method, state) in [
        ("wake/send", "active"),
        ("wake/pause", "paused"),
        ("wake/resume", "active"),
        ("wake/cancel", "cancelled"),
    ] {
        let operation = OperationId::generate();
        let mut answer = original.clone();
        answer
            .as_object_mut()
            .ok_or("answer object")?
            .insert("state".into(), json!(state));
        if state != "active" {
            answer
                .as_object_mut()
                .ok_or("answer object")?
                .insert("nextDueAtMs".into(), Value::Null);
        }
        let answer = if method == "wake/send" {
            answer
        } else {
            json!({"observed_at_ms":now+1,"wake":answer,"discarded":[],"retained":[]})
        };
        sqlx::query("INSERT INTO operation_receipts(operation_id,method_name,canonical_request,resource_id,operation_status,effect_evidence_json,final_result_json,final_error_json,committed_at_ms) VALUES (?,?,?,?,'succeeded','{\"kind\":\"local\",\"mutation\":\"committed\"}',?,NULL,?)")
            .bind(operation.as_str()).bind(method).bind(&canonical).bind(id.as_str()).bind(answer.to_string()).bind(now).execute(&mut connection).await?;
        cases.push((operation, method, answer));
    }
    connection.close().await?;
    let mut store = AutomationStore::open(&path).await?;
    for (operation, method, original) in &cases {
        let receipt = store.read_operation(operation).await?;
        let automation_storage::StoredOperationState::Succeeded { result } = receipt.state else {
            return Err("original operation answer lost".into());
        };
        if *method == "wake/send" {
            let decoded = serde_json::from_value::<
                agent_automation::WakeRecord<SavedMessage, WakeCreator>,
            >(result.clone());
            ensure!(
                decoded.is_ok(),
                "upgrade made the original operation answer unreadable: {decoded:?}"
            );
        } else {
            let decoded = serde_json::from_value::<
                automation_storage::WakeMutationResult<SavedMessage>,
            >(result.clone());
            ensure!(
                decoded.is_ok(),
                "upgrade made the original mutation answer unreadable: {method}: {decoded:?}"
            );
        }
        let mut expected = original.clone();
        let path = if *method == "wake/send" {
            "/definition"
        } else {
            "/wake/definition"
        };
        expected
            .pointer_mut(path)
            .and_then(Value::as_object_mut)
            .ok_or("definition missing")?
            .insert("createdBy".into(), json!({"kind":"unrecorded"}));
        ensure_eq!(
            result,
            expected,
            "upgrade rewrote original operation outcome or timing: {method}"
        );
    }
    store.close().await?;
    let mut connection =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    for (operation, _, _) in cases {
        let retained: Vec<u8> = sqlx::query_scalar(
            "SELECT canonical_request FROM operation_receipts WHERE operation_id=?",
        )
        .bind(operation.as_str())
        .fetch_one(&mut connection)
        .await?;
        ensure_eq!(retained, canonical);
    }
    connection.close().await?;
    std::fs::remove_file(path)?;
    Ok(())
}
