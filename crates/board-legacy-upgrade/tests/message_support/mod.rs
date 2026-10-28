#![expect(
    dead_code,
    reason = "Shared fixture helpers serve different subsets of integration targets."
)]
use rusqlite::{Connection, params};
use serde_json::json;
use sqlx::Connection as _;
use std::path::Path;
pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
pub const BEFORE_DROP: i64 = 20_261_019_000_200;
pub const DROP_VERSION: i64 = 20_261_021_000_100;

pub async fn source(path: &Path, version: i64) -> TestResult {
    let mut connection = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true),
    )
    .await?;
    let migrations = sqlx::migrate::Migrator::new(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../automation-storage/migrations"),
    )
    .await?;
    migrations
        .run_direct(Some(version), &mut connection, false)
        .await?;
    connection.close().await?;
    Ok(())
}

pub async fn seeded_source(path: &Path) -> TestResult<Vec<String>> {
    source(path, BEFORE_DROP).await?;
    let connection = Connection::open(path)?;
    let ids = vec![
        uuid::Uuid::now_v7().to_string(),
        uuid::Uuid::now_v7().to_string(),
    ];
    let receipt = collaboration_protocol::DeliveryReceipt {
        outcome: collaboration_protocol::DeliveryOutcome::NotSubmitted {
            retryable: true,
            reason: "fixture target is absent".into(),
        },
        reachability: None,
        client: None,
    };
    for (index, id) in ids.iter().enumerate() {
        let (state, outcome) = if index == 0 {
            ("pending", None)
        } else {
            ("held", Some(serde_json::to_string(&receipt)?))
        };
        connection.execute("INSERT INTO router_pushes(push_id,kind,origin_kind,target_service_id,target_endpoint_id,target_session_id,dm_delivery_mode,header_facts_json,body,delivery_state,last_outcome_json,created_at) VALUES (?,'direct-message','owner','550e8400-e29b-41d4-a716-446655440000','codex-local','legacy-recipient','auto',?, ?, ?, ?, '2026-10-21T12:34:56.123456789Z')", params![id, " {\"kind\": \"directMessage\", \"senderDisplayName\":null} ", "  preserved message\n", state, outcome])?;
    }
    let project = uuid::Uuid::now_v7().to_string();
    let membership = uuid::Uuid::now_v7().to_string();
    let payload = json!({
        "project":project,"membership":membership,"generation":1,
        "activatedAt":{"epoch":1,"position":1},"scanThrough":{"epoch":2,"position":6},"lastOutcome":null,
        "roots":[{"threadId":uuid::Uuid::now_v7(),"deliveredThrough":{"epoch":1,"position":2},"window":{
            "windowId":uuid::Uuid::now_v7(),"from":{"epoch":1,"position":3},"through":{"epoch":2,"position":6},
            "postCount":2,"openedAt":"2026-10-21T12:34:56Z","lastArrivalAt":"2026-10-21T12:34:57Z",
            "selected":null,"heldSince":"2026-10-21T12:35:00Z","nextRetryAt":"2026-10-21T12:36:00Z","retryAttempts":3
        }}]
    });
    connection.execute(
        "INSERT INTO subscription_delivery_progress VALUES (?,?,1,1,?)",
        params![project, membership, serde_json::to_string(&payload)?],
    )?;
    connection.execute(
        "INSERT INTO board_intent_progress VALUES (?,?,2,5)",
        params![project, uuid::Uuid::now_v7().to_string()],
    )?;
    connection.execute(
        "INSERT INTO board_intent_scan_cursors VALUES (?,2,6)",
        params![project],
    )?;
    connection.close().map_err(|(_, error)| error)?;
    Ok(ids)
}

pub fn text_rows(path: &Path, sql: &str) -> TestResult<Vec<String>> {
    let connection = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let result = connection
        .prepare(sql)?
        .query_map([], |row| row.get("value"))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(result)
}
pub const IDS: &str = "SELECT progress_id AS value FROM subscription_delivery_progress UNION ALL SELECT progress_id AS value FROM board_intent_progress ORDER BY value";
pub const RANGE_ROOT: &str = "019a0000-0000-7000-8000-000000000065";
pub const RANGE_PROJECT: &str = "019a0000-0000-7000-8000-000000000015";
pub const RANGES: &str =
    "SELECT ranges_json AS value FROM router_pushes WHERE ranges_json IS NOT NULL ORDER BY push_id";

pub fn activity_push(path: &Path, root: &str, from: u64, through: u64) -> TestResult<String> {
    let target = serde_json::from_value(
        json!({"endpoint":{"serviceId":"550e8400-e29b-41d4-a716-446655440000","endpointId":"codex-local"},"sessionId":"legacy-recipient"}),
    )?;
    let origin = collaboration_protocol::RouterOriginRef::SubscriptionActivity {
        target,
        batch_id: uuid::Uuid::now_v7().to_string().try_into()?,
    }
    .canonical_string()?;
    let id = uuid::Uuid::now_v7().to_string();
    let connection = Connection::open(path)?;
    connection.execute("INSERT INTO router_pushes(push_id,kind,origin_kind,origin_router_ref,target_service_id,target_endpoint_id,target_session_id,header_facts_json,ranges_json,delivery_state,created_at) VALUES (?,'subscription-activity','router',?,'550e8400-e29b-41d4-a716-446655440000','codex-local','legacy-recipient',?,?,'pending','2026-10-21T12:34:56Z')", params![id, origin,
        json!({"kind":"subscriptionActivity","rootCount":1,"messageCount":1,"heldSince":null,"threadResolved":false}).to_string(),
        json!({"ranges":[{"rootMessageId":root,"fromActivitySequence":from,"throughActivitySequence":through}],"held":false,"draining":true}).to_string()])?;
    Ok(id)
}
pub const ORIGINS: &str = "SELECT origin_router_ref AS value FROM router_pushes WHERE origin_router_ref IS NOT NULL ORDER BY push_id";
pub fn expiry_push(path: &Path, scope: serde_json::Value) -> TestResult<String> {
    let target: collaboration_protocol::SessionRef = serde_json::from_value(
        json!({"endpoint":{"serviceId":"550e8400-e29b-41d4-a716-446655440000","endpointId":"codex-local"},"sessionId":"legacy-recipient"}),
    )?;
    let (kind, id) = if let Some(id) = scope
        .get("rootMessageId")
        .and_then(serde_json::Value::as_str)
    {
        ("thread", id)
    } else {
        (
            "topic",
            scope
                .get("topicId")
                .and_then(serde_json::Value::as_str)
                .ok_or("fixture scope missing")?,
        )
    };
    let argument = if kind == "thread" {
        "--root-message-id"
    } else {
        "--topic-id"
    };
    // Match the shipped enum's declaration order, including the typed SessionRef.
    let origin = format!(
        "{{\"kind\":\"subscriptionExpiry\",\"target\":{},\"scope\":{},\"subscriptionGeneration\":1}}",
        serde_json::to_string(&target)?,
        scope
    );
    let id_value = uuid::Uuid::now_v7().to_string();
    let connection = Connection::open(path)?;
    connection.execute("INSERT INTO router_pushes(push_id,kind,origin_kind,origin_router_ref,target_service_id,target_endpoint_id,target_session_id,header_facts_json,body,delivery_state,created_at) VALUES (?,'subscription-expiry','router',?,'550e8400-e29b-41d4-a716-446655440000','codex-local','legacy-recipient',?,?,'pending','2026-10-21T12:34:56Z')", params![id_value, origin,
        json!({"kind":"subscriptionExpiry","scope":format!("{kind} {id}")}).to_string(),
        format!("Renew with: agent-collaboration board thread subscribe {argument} {id}")])?;
    Ok(id_value)
}
pub const HISTORY: &str = "SELECT json_object('id',request_id,'record',record_json,'created',created_at) AS value FROM typed_interaction_history UNION ALL SELECT json_object('revision',revision) AS value FROM interaction_history_revision ORDER BY value";
pub fn source_tables(path: &Path) -> TestResult<Vec<String>> {
    text_rows(
        path,
        "SELECT name AS value FROM sqlite_schema WHERE type='table' AND name IN ('router_pushes','subscription_delivery_progress','board_intent_progress','board_intent_scan_cursors','mail_items') ORDER BY name",
    )
}
pub async fn history(path: &Path) -> TestResult<Vec<String>> {
    use interaction_storage::{
        InteractionHistoryStore, InteractionStore, RefusedApprovalOption, RefusedTypedApproval,
    };
    let owner = std::sync::Arc::new(InteractionStore::open(path).await?);
    let history = InteractionHistoryStore::from_owner(owner.clone()).await?;
    let requester = serde_json::from_value(
        json!({"endpoint":{"serviceId":"550e8400-e29b-41d4-a716-446655440000","endpointId":"codex-local"},"sessionId":"history-requester"}),
    )?;
    history
        .record_refused_approval(
            requester,
            message_board::Identity::Human {
                human_id: "fixture owner".to_owned().try_into()?,
            },
            RefusedTypedApproval {
                request_id: uuid::Uuid::now_v7().to_string(),
                title: "Preserve existing typed history".into(),
                description: None,
                subject: None,
                options: vec![RefusedApprovalOption {
                    option_id: "reject".into(),
                    label: "Reject".into(),
                    provider_kind: "fixture".into(),
                }],
                reason: "fixture refusal".into(),
            },
        )
        .await?;
    drop(history);
    std::sync::Arc::try_unwrap(owner)
        .map_err(|_| "history retained its owner")?
        .close()
        .await?;
    text_rows(path, HISTORY)
}
