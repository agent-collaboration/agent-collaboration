use collaboration_client::WakeWaitError;
use collaboration_protocol::{OperationId, WakeMutationRequest, WakeSendRequest, WakeShowRequest};
use serde_json::json;
#[path = "support/central_catalog_fixture.rs"]
mod fixture;
#[path = "support/legacy_wake.rs"]
mod legacy;
use fixture::{CatalogFixture, ProofResult};
#[path = "support/wake_push_draft.rs"]
mod wake_push_test_support;
#[tokio::test]
async fn dedicated_wait_observes_pause_after_immediate_resume() -> ProofResult {
    let fixture = CatalogFixture::open(|transport| transport, true).await?;
    let served = &fixture.api;
    let store = fixture
        .store
        .as_ref()
        .ok_or("local state fixture missing")?;
    let client = served.client("wake-control").await?;
    let request: WakeSendRequest = serde_json::from_value(
        json!({"operationId":OperationId::generate(),"message":{"target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"fixture-only"},"content":{"kind":"humanUser","text":"Check"},"delivery":"auto","generationGuard":null},"timing":{"kind":"interval","seconds":300},"requestedBy":{"kind":"owner"},"replace":false,"expiry":{"kind":"after","seconds":7200}}),
    )?;
    let id = legacy::seed(store, request).await?;
    let wake = legacy::snapshot(
        client
            .read_wakeup(WakeShowRequest { wakeup_id: id })
            .await?,
    )?;
    let wait = served
        .client("wake-wait")
        .await?
        .subscribe_wakeup(WakeShowRequest {
            wakeup_id: wake.definition.wakeup_id.clone(),
        })
        .await?;
    client
        .pause_wakeup(WakeMutationRequest {
            operation_id: OperationId::generate(),
            wakeup_id: wake.definition.wakeup_id.clone(),
        })
        .await?;
    client
        .resume_wakeup(WakeMutationRequest {
            operation_id: OperationId::generate(),
            wakeup_id: wake.definition.wakeup_id.clone(),
        })
        .await?;
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        wait.wait_until_first_fire(),
    )
    .await?;
    if !matches!(outcome, Err(WakeWaitError::Paused { .. })) {
        return Err("wait missed pause during rapid resume".into());
    }
    store
        .lock()
        .await
        .evaluate_wakeup::<collaboration_protocol::SavedMessage>(
            &wake.definition.wakeup_id,
            chrono::Utc::now().timestamp_millis() + 300000,
            wake_push_test_support::build_test_wake_push_draft,
        )
        .await?;
    client
        .cancel_wakeup(WakeMutationRequest {
            operation_id: OperationId::generate(),
            wakeup_id: wake.definition.wakeup_id.clone(),
        })
        .await?;
    let historical = served
        .client("wake-history")
        .await?
        .subscribe_wakeup(WakeShowRequest {
            wakeup_id: wake.definition.wakeup_id.clone(),
        })
        .await?
        .wait_until_first_fire()
        .await?;
    let collaboration_client::WakeFirstFireResponse::Local { value: historical } = historical
    else {
        return Err("historical local first fire lost its actual receipt".into());
    };
    if historical.wakeup_id != wake.definition.wakeup_id {
        return Err("historical firing identity changed".into());
    }
    let missing = served
        .client("wake-missing")
        .await?
        .subscribe_wakeup(WakeShowRequest {
            wakeup_id: collaboration_protocol::WakeupId::generate(),
        })
        .await;
    if !matches!(missing, Err(WakeWaitError::NotFound { .. })) {
        return Err("missing wake was confused with unavailable observation".into());
    }
    fixture.close().await
}
