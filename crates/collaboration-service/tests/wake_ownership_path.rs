//! Unmoved local Wakes keep their real SQLite controls through the public API.
use automation_storage::{AutomationStore, WakeCreate};
use collaboration_client::{WakeClientError, WakeMutationResponse, WakeSelectionFailure};
use collaboration_protocol::{OperationId, SavedMessage, WakeMutationRequest, WakeState, WakeupId};
use collaboration_service::ApplicationFixture;
use std::{os::unix::fs::PermissionsExt, sync::Arc};
use tokio::sync::Mutex;
#[path = "support/served_api.rs"]
mod served_api;
type ProofResult<Value = ()> = Result<Value, Box<dyn std::error::Error>>;
const SERVICE: &str = "00000000-0000-4000-8000-000000000001";
const EPOCH: &str = "00000000-0000-4000-8000-000000000002";

async fn seed_local_wake(store: &Arc<Mutex<AutomationStore>>) -> ProofResult<WakeupId> {
    let message: SavedMessage = serde_json::from_value(serde_json::json!({
        "target":{"endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},"sessionId":"inert-unmoved-wake"},
        "content":{"kind":"router","text":"No native backend or input"},"delivery":"queue"
    }))?;
    Ok(store
        .lock()
        .await
        .create_wakeup(&WakeCreate {
            operation_id: OperationId::generate(),
            requested_by: collaboration_protocol::WakeCreator::Router {},
            replace: false,
            message,
            timing: agent_automation::TimingRule::After { seconds: 3600 },
            expiry: agent_automation::ExpiryRule::After { seconds: 7200 },
            now_ms: chrono::Utc::now().timestamp_millis(),
        })
        .await?
        .definition
        .wakeup_id)
}

#[tokio::test]
async fn unmoved_and_frozen_local_reads_preserve_the_legacy_snapshot_without_central() -> ProofResult
{
    use sqlx::Connection;
    let directory = tempfile::Builder::new()
        .prefix("s6-local-read-")
        .tempdir()?;
    let path = directory.path().join("automation.sqlite");
    let store = Arc::new(Mutex::new(AutomationStore::open(&path).await?));
    let wakeup_id = seed_local_wake(&store).await?;
    let served = served_api::ServedApi::start(
        ApplicationFixture::new(SERVICE, EPOCH)?.with_automation_store(store.clone()),
    )?;
    let proof: ProofResult = async {
        let client = served.client("local-wake-read").await?;
        let request = collaboration_protocol::WakeShowRequest { wakeup_id: wakeup_id.clone() };
        let before = serde_json::to_value(client.read_wakeup(request.clone()).await?)?;
        if before["kind"] != "local" || before.pointer("/value/definition/wakeupId") != Some(&serde_json::json!(wakeup_id))
            || before.pointer("/value/latestEventCursor").and_then(serde_json::Value::as_str).is_none_or(str::is_empty) {
            return Err("unmoved local read lost its Local tag, identity or actual event cursor".into());
        }
        let mut sql = sqlx::SqliteConnection::connect_with(&sqlx::sqlite::SqliteConnectOptions::new().filename(&path)).await?;
        // The marker stands in only for S7's producer, whose real freeze/import remains unverified.
        sqlx::query("INSERT INTO automation_handover(automation_id,state,watermark_local_ms,frozen_payload,digest) VALUES (?,'pending',0,?,?)")
            .bind(wakeup_id.as_str()).bind(b"{}".as_slice()).bind("44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a").execute(&mut sql).await?;
        sql.close().await?;
        let frozen = serde_json::to_value(client.read_wakeup(request).await?)?;
        if frozen["kind"] != "local" || frozen.pointer("/value/definition") != before.pointer("/value/definition")
            || frozen.pointer("/value/state") != before.pointer("/value/state") {
            return Err("frozen local Wake stopped being readable or changed its definition/state".into());
        }
        Ok(())
    }.await;
    let stopped = served.stop().await;
    drop(store);
    let removed = directory.close();
    proof?;
    stopped?;
    removed?;
    Ok(())
}

#[tokio::test]
async fn unmoved_local_controls_replay_with_legacy_snapshot_and_no_central_capability()
-> ProofResult {
    let directory = tempfile::Builder::new()
        .prefix("s6-local-wake-")
        .tempdir()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let store = Arc::new(Mutex::new(
        AutomationStore::open(&directory.path().join("automation.sqlite")).await?,
    ));
    let wakeup_id = seed_local_wake(&store).await?;
    let served = served_api::ServedApi::start(
        ApplicationFixture::new(SERVICE, EPOCH)?.with_automation_store(store.clone()),
    )?;
    let proof: ProofResult = async {
        let client = served.client("local-wake-controls").await?;
        let request = WakeMutationRequest {
            operation_id: OperationId::generate(),
            wakeup_id: wakeup_id.clone(),
        };
        // Act: this application has no catalog transport; only the durable local owner can succeed.
        let pause = client.pause_wakeup(request.clone()).await?;
        let replay = client.pause_wakeup(request).await?;
        if serde_json::to_value(&pause)? != serde_json::to_value(replay)? {
            return Err("local pause replay changed its original snapshot/cursor".into());
        }
        let WakeMutationResponse::Local(paused) = pause else {
            return Err("unmoved local pause was routed to central".into());
        };
        let anchor = paused.wakeup.definition.anchor_at.clone();
        if paused.wakeup.state != WakeState::Paused
            || paused.wakeup.latest_event_cursor.is_empty()
            || paused.wakeup.first_fire.is_some()
            || paused.wakeup.pending_delivery_id.is_some()
        {
            return Err(
                "local pause lost its real cursor/state or invented firing evidence".into(),
            );
        }
        let WakeMutationResponse::Local(resumed) = client
            .resume_wakeup(WakeMutationRequest {
                operation_id: OperationId::generate(),
                wakeup_id: wakeup_id.clone(),
            })
            .await?
        else {
            return Err("unmoved local resume was routed to central".into());
        };
        if resumed.wakeup.state != WakeState::Active
            || resumed.wakeup.definition.anchor_at != anchor
        {
            return Err("local resume changed the original timing anchor".into());
        }
        let WakeMutationResponse::Local(cancelled) = client
            .cancel_wakeup(WakeMutationRequest {
                operation_id: OperationId::generate(),
                wakeup_id: wakeup_id.clone(),
            })
            .await?
        else {
            return Err("unmoved local cancel was routed to central".into());
        };
        if cancelled.wakeup.state != WakeState::Cancelled
            || cancelled.wakeup.definition.wakeup_id != wakeup_id
            || cancelled.wakeup.latest_event_cursor.is_empty()
        {
            return Err("local cancel lost its identity, state or event cursor".into());
        }
        Ok(())
    }
    .await;
    let stopped = served.stop().await;
    drop(store);
    let removed = directory.close();
    proof?;
    stopped?;
    removed?;
    Ok(())
}

#[tokio::test]
async fn corrupt_ownership_is_a_typed_selection_failure_without_central_fallback() -> ProofResult {
    use sqlx::Connection;
    let directory = tempfile::Builder::new()
        .prefix("s6-owner-error-")
        .tempdir()?;
    let path = directory.path().join("automation.sqlite");
    let store = Arc::new(Mutex::new(AutomationStore::open(&path).await?));
    let wakeup_id = seed_local_wake(&store).await?;
    let mut sql = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(&path),
    )
    .await?;
    sqlx::query("UPDATE wakeup_definitions SET authority='invalid' WHERE wakeup_id=?")
        .bind(wakeup_id.as_str())
        .execute(&mut sql)
        .await?;
    sql.close().await?;
    let served = served_api::ServedApi::start(
        ApplicationFixture::new(SERVICE, EPOCH)?.with_automation_store(store.clone()),
    )?;
    let proof: ProofResult = async {
        let client = served.client("ownership-selection-error").await?;
        let operation = OperationId::generate();
        let failed = client.pause_wakeup(WakeMutationRequest { operation_id: operation.clone(), wakeup_id: wakeup_id.clone() }).await;
        // A guessed central path would return CentralUnreachable here, losing the selection error.
        if !matches!(failed, Err(WakeClientError::Selection(error)) if matches!(&*error,
            WakeSelectionFailure::LocalStoreUnavailable { operation_id: Some(actual), wakeup_id: Some(wake), .. }
                if actual == &operation && wake == &wakeup_id)) {
            return Err("invalid local authority was guessed as central or lost typed request identity".into());
        }
        Ok(())
    }.await;
    let stopped = served.stop().await;
    drop(store);
    let removed = directory.close();
    proof?;
    stopped?;
    removed?;
    Ok(())
}

#[tokio::test]
async fn missing_ownership_store_does_not_guess_an_owner() -> ProofResult {
    let served = served_api::ServedApi::start(ApplicationFixture::new(SERVICE, EPOCH)?)?;
    let proof: ProofResult = async {
        let client = served.client("ownership-store-absent").await?;
        let operation = OperationId::generate();
        let wakeup_id = WakeupId::generate();
        let failed = client.cancel_wakeup(WakeMutationRequest { operation_id: operation.clone(), wakeup_id: wakeup_id.clone() }).await;
        if !matches!(failed, Err(WakeClientError::Selection(error)) if matches!(&*error,
            WakeSelectionFailure::LocalStoreUnavailable { operation_id: Some(actual), wakeup_id: Some(wake), .. }
                if actual == &operation && wake == &wakeup_id)) {
            return Err("missing local ownership storage was guessed as central".into());
        }
        Ok(())
    }.await;
    let stopped = served.stop().await;
    proof?;
    stopped?;
    Ok(())
}
