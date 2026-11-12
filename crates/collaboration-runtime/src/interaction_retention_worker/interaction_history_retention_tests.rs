//! Retention across interaction history rows and the interaction store's approval routes.
use super::InteractionRetentionWorker;
use super::retention_test_harness::{insert_route, route_ids};
use crate::{
    InteractionHistoryRecord, NativeControlBackend, NativeGenerationGate, RefusedApprovalOption,
    RefusedTypedApproval, ServiceInteractionBroker,
};
use chrono::{DateTime, SecondsFormat, Utc};
use collaboration_protocol::{EndpointId, EndpointRef, SessionId, SessionRef, UuidIdentity};
use serde_json::{Value, json};
use sqlx::Connection;
use sqlx_turso::{TursoConnectOptions, TursoConnection};
use std::{collections::BTreeMap, path::Path, sync::Arc};

pub(super) type TestResult<TValue = ()> = Result<TValue, Box<dyn std::error::Error + Send + Sync>>;
const SERVICE_ID: &str = "018f47d2-24d5-7a68-b9ec-6f759c39458f";
const INITIAL_REQUEST: &str = "initial-interaction";

fn ensure(condition: bool, detail: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(detail.into())
    }
}

pub(super) fn session(session_id: &str) -> TestResult<SessionRef> {
    Ok(SessionRef {
        endpoint: EndpointRef {
            service_id: UuidIdentity::try_from(SERVICE_ID.to_owned())?,
            endpoint_id: EndpointId::try_from("codex-local".to_owned())?,
        },
        session_id: SessionId::try_from(session_id.to_owned())?,
    })
}

pub(super) fn history_value(
    request_id: &str,
    body: &str,
    created_at: Option<DateTime<Utc>>,
) -> TestResult<Value> {
    let requester = serde_json::from_value(serde_json::to_value(session("requester")?)?)?;
    let approver = serde_json::from_value(json!({"kind":"human", "humanId":"retention-owner"}))?;
    let record = InteractionHistoryRecord::RefusedApproval {
        requester,
        approver,
        refusal: RefusedTypedApproval {
            request_id: request_id.to_owned(),
            title: body.to_owned(),
            description: Some(body.to_owned()),
            subject: None,
            options: vec![RefusedApprovalOption {
                option_id: "reject".to_owned(),
                label: "Reject".to_owned(),
                provider_kind: "fixture".to_owned(),
            }],
            reason: "fixture refusal".to_owned(),
        },
    };
    let mut value = serde_json::to_value(record)?;
    if let Some(created_at) = created_at {
        value
            .as_object_mut()
            .ok_or("history record must encode an object")?
            .insert(
                "createdAt".to_owned(),
                Value::String(created_at.to_rfc3339_opts(SecondsFormat::Nanos, true)),
            );
    }
    Ok(value)
}

async fn read_history(path: &Path) -> TestResult<BTreeMap<String, Value>> {
    let mut connection =
        TursoConnection::connect_with(&TursoConnectOptions::new().filename(path).read_only(true))
            .await?;
    let rows: Vec<(String, String, String)> = sqlx::query_as("SELECT request_id, record_json, created_at FROM typed_interaction_history ORDER BY request_id")
        .fetch_all(&mut connection).await?;
    let mut records = BTreeMap::new();
    for (request_id, record_json, created_at) in rows {
        let mut record: Value = serde_json::from_str(&record_json)?;
        record
            .as_object_mut()
            .ok_or("stored record must be an object")?
            .insert("createdAt".into(), Value::String(created_at));
        records.insert(request_id, record);
    }
    Ok(records)
}

fn creation_stamp(history: &BTreeMap<String, Value>) -> TestResult<String> {
    Ok(history
        .get(INITIAL_REQUEST)
        .and_then(|record| record.get("createdAt"))
        .and_then(Value::as_str)
        .ok_or("initial history must contain its persisted creation stamp")?
        .to_owned())
}

pub(super) async fn load_broker(
    directory: &Path,
    gate: &NativeGenerationGate,
) -> TestResult<Arc<ServiceInteractionBroker>> {
    ensure(
        gate.acquire().is_err(),
        "retention fixture native gate must stay inactive",
    )?;
    let interaction_store = Arc::new(
        interaction_storage::InteractionStore::open(directory.join("interaction.sqlite")).await?,
    );
    Ok(ServiceInteractionBroker::load(
        UuidIdentity::try_from(SERVICE_ID.to_owned())?,
        NativeControlBackend {
            endpoint: session("unused-backend")?.endpoint,
            gate: gate.clone(),
            codex_home: directory.join("unused-codex-home"),
        },
        interaction_store,
    )
    .await?)
}

async fn ensure_history_contents(
    broker: &ServiceInteractionBroker,
    path: &Path,
    expected: &[&str],
) -> TestResult {
    let mut memory_ids = broker
        .list_interactions()
        .await
        .iter()
        .map(|record| record.request_id().to_owned())
        .collect::<Vec<_>>();
    let disk = read_history(path).await?;
    let disk_ids = disk.keys().cloned().collect::<Vec<_>>();
    let mut expected_ids = expected
        .iter()
        .map(|id| (*id).to_owned())
        .collect::<Vec<_>>();
    memory_ids.sort();
    expected_ids.sort();
    ensure(
        memory_ids == expected_ids,
        "worker must prune broker's actual in-memory interaction history",
    )?;
    ensure(
        disk_ids == expected_ids,
        "worker must remove expired history records and bodies from native message storage",
    )
}

#[tokio::test]
async fn retention_worker_prunes_real_broker_history_and_sqlite_payloads_with_one_persisted_creation_stamp()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let history_path = directory.path().join("interaction.sqlite");
    let gate = NativeGenerationGate::default();
    let first_broker = load_broker(directory.path(), &gate).await?;
    drop(first_broker);
    let mut seed =
        TursoConnection::connect_with(&TursoConnectOptions::new().filename(&history_path)).await?;
    let initial_record = history_value(INITIAL_REQUEST, "initial-history-body", None)?;
    let initial_stamp = "2026-01-15T08:00:00.123456789Z"
        .parse::<DateTime<Utc>>()?
        .to_rfc3339_opts(SecondsFormat::Nanos, true);
    let mut transaction = seed.begin().await?;
    sqlx::query(
        "INSERT INTO typed_interaction_history (request_id,record_json,created_at) VALUES (?,?,?)",
    )
    .bind(INITIAL_REQUEST)
    .bind(serde_json::to_string(&initial_record)?)
    .bind(initial_stamp)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("UPDATE interaction_history_revision SET revision=revision+1")
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    seed.close().await?;
    let first_broker = load_broker(directory.path(), &gate).await?;
    let mut persisted = read_history(&history_path).await?;
    let persisted_stamp = creation_stamp(&persisted)?;
    let stamped_at = DateTime::parse_from_rfc3339(&persisted_stamp)?.with_timezone(&Utc);
    ensure_history_contents(&first_broker, &history_path, &[INITIAL_REQUEST]).await?;
    drop(first_broker);

    // The worker accepts milliseconds; the observed nanosecond stamp remains
    // on or after this floored cutoff and before the following millisecond.
    let cutoff_ms = stamped_at.timestamp_millis();
    let cutoff = DateTime::<Utc>::from_timestamp_millis(cutoff_ms).ok_or("valid cutoff")?;
    let expired =
        DateTime::<Utc>::from_timestamp_millis(cutoff_ms - 1).ok_or("valid expired time")?;
    let recent =
        DateTime::<Utc>::from_timestamp_millis(cutoff_ms + 1).ok_or("valid recent time")?;
    for (id, body, created_at) in [
        ("expired-interaction", "expired-history-body", expired),
        ("cutoff-interaction", "cutoff-history-body", cutoff),
        ("recent-interaction", "recent-history-body", recent),
    ] {
        persisted.insert(id.to_owned(), history_value(id, body, Some(created_at))?);
    }
    let mut history_seed =
        TursoConnection::connect_with(&TursoConnectOptions::new().filename(&history_path)).await?;
    let mut transaction = history_seed.begin().await?;
    for (request_id, mut value) in persisted.clone() {
        if request_id == INITIAL_REQUEST {
            continue;
        }
        let created_at = value
            .as_object_mut()
            .ok_or("fixture record")?
            .remove("createdAt")
            .ok_or("fixture timestamp")?
            .as_str()
            .ok_or("timestamp text")?
            .to_owned();
        sqlx::query("INSERT INTO typed_interaction_history (request_id,record_json,created_at) VALUES (?,?,?)")
            .bind(request_id).bind(serde_json::to_string(&value)?).bind(created_at)
            .execute(&mut *transaction).await?;
    }
    sqlx::query("UPDATE interaction_history_revision SET revision=revision+1")
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    history_seed.close().await?;

    let broker = load_broker(directory.path(), &gate).await?;
    ensure(
        creation_stamp(&read_history(&history_path).await?)? == persisted_stamp,
        "broker reload must preserve the original creation stamp",
    )?;

    let native = broker.interaction_store();
    let expired_route = DateTime::<Utc>::from_timestamp_millis(cutoff_ms - 1).ok_or("expired")?;
    insert_route(&native, "expired-route", expired_route).await;
    insert_route(&native, "cutoff-route", cutoff).await;
    insert_route(&native, "recent-route", recent).await;
    let worker = InteractionRetentionWorker::new(std::sync::Arc::new(
        router_clock::ManualClock::starting_at(runtime_primitives::SyncTime::from_unix_millis(
            1792432800000,
        ))
        .unwrap(),
    ))
    .with_interaction_broker(broker.clone())
    .with_interaction_store(native.clone());
    let first_maintenance_ms = DateTime::<Utc>::from_timestamp_millis(cutoff_ms)
        .ok_or("valid retained cutoff")?
        .checked_add_months(chrono::Months::new(2))
        .ok_or("valid two-month deadline")?
        .timestamp_millis();

    ensure(
        worker.prune_batch(first_maintenance_ms).await == 2,
        "worker must prune the expired route and interaction independently",
    )?;
    ensure(
        route_ids(directory.path()).await == ["cutoff-route", "recent-route"],
        "retention must remove only the expired approval route",
    )?;
    ensure_history_contents(
        &broker,
        &history_path,
        &["cutoff-interaction", INITIAL_REQUEST, "recent-interaction"],
    )
    .await?;
    ensure(
        creation_stamp(&read_history(&history_path).await?)? == persisted_stamp,
        "retention must preserve the retained creation stamp",
    )?;
    ensure(
        !serde_json::to_string(&read_history(&history_path).await?)?
            .contains("expired-history-body"),
        "expired interaction body must be absent from durable active rows",
    )?;

    ensure(
        worker.prune_batch(first_maintenance_ms + 1).await == 3,
        "creation stamp must age after two calendar months, without reload restamping",
    )?;
    ensure(
        route_ids(directory.path()).await == ["recent-route"],
        "the cutoff route goes one millisecond later",
    )?;
    ensure_history_contents(&broker, &history_path, &["recent-interaction"]).await?;
    ensure(
        !serde_json::to_string(&read_history(&history_path).await?)?
            .contains("initial-history-body"),
        "aged initial interaction body must be absent from durable active rows",
    )?;
    ensure(
        gate.acquire().is_err(),
        "retention must not activate a native generation",
    )?;

    drop(worker);
    drop(broker);
    Arc::try_unwrap(native)
        .map_err(|_| "retention fixture retained the native owner")?
        .close()
        .await?;
    Ok(())
}
