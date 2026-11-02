//! Real public central commands and retained local delivery history have separate owners.
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, OperationId, WakeSendRequest, WakeShowRequest,
};
use serde_json::json;
#[path = "support/central_catalog_fixture.rs"]
mod fixture;
#[path = "support/legacy_wake.rs"]
mod legacy;
#[path = "support/wake_push_draft.rs"]
mod wake_push_test_support;
use fixture::{CatalogFixture, ProofResult, SERVICE};

#[tokio::test]
async fn real_control_client_preserves_wake_identity_timing_and_message() -> ProofResult {
    let fixture = CatalogFixture::open_with_controls(|transport| transport, true, vec![serde_json::from_value(json!({
        "address":{"kind":"fleet"},"command":{"kind":"automationClock","value":{"kind":"start","now":"2030-01-01T00:00:00Z"}}
    }))?]).await?;
    let proof: ProofResult = async {
        let ready = fixture.transport.call(AutomationCall::PullAutomationBatches(collaboration_protocol::PullAutomationBatches {
            after: None, max_bytes: 1_048_576, work_protocols: vec![collaboration_protocol::MachineWorkProtocol::MachineWorkV1],
        })).await?;
        if !matches!(ready,AutomationAnswer::Batches(_)) { return Err("target capability missing".into()); }
        let client = fixture.api.client("central-wake-test").await?;
        let target = json!({"endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},"sessionId":"fixture-only-thread"});
        let at = "2030-01-01T00:05:00Z";
        let request: WakeSendRequest = serde_json::from_value(json!({
            "operationId":OperationId::generate(),"message":{"target":target,"content":{"kind":"agent","sender":target,"text":"Check status"},"delivery":"queue","generationGuard":null},
            "timing":{"kind":"at","at":at},"requestedBy":{"kind":"owner"},"replace":false,"expiry":{"kind":"after","seconds":7200}
        }))?;
        let first = central_create(client.send_wakeup(request.clone()).await?)?;
        let replay = central_create(client.send_wakeup(request).await?)?;
        let current = central_read(client.read_wakeup(WakeShowRequest { wakeup_id:first.definition.wakeup_id.clone() }).await?)?;
        if serde_json::to_value(&first)? != serde_json::to_value(replay)? || serde_json::to_value(&first.definition)? != serde_json::to_value(&current.definition)? {
            return Err("wake replay or inspection changed its definition".into());
        }
        if current.first_firing.is_some() || serde_json::to_value(&current)?.get("pendingDeliveryId").is_some() {
            return Err("creation fabricated firing or native acceptance".into());
        }
        let listed = client.list_wakeups(collaboration_protocol::AutomationPageRequest { cursor:None,limit:50_u32.try_into()? }).await?;
        if listed.records.len()!=1 || central_read(listed.records.into_iter().next().ok_or("listed Wake missing")?)?.definition.wakeup_id != first.definition.wakeup_id {
            return Err("SDK wake list omitted created wake".into());
        }
        let pause_request = collaboration_protocol::WakeMutationRequest { operation_id:OperationId::generate(),wakeup_id:first.definition.wakeup_id.clone() };
        let paused = client.pause_wakeup(pause_request.clone()).await?;
        let replayed_pause = client.pause_wakeup(pause_request).await?;
        if serde_json::to_value(&paused)? != serde_json::to_value(replayed_pause)? { return Err("mutation replay changed the original receipt".into()); }
        if central_mutation(paused)?.state != collaboration_protocol::WakeState::Paused { return Err("pause did not reach central".into()); }
        let resumed = central_mutation(client.resume_wakeup(collaboration_protocol::WakeMutationRequest { operation_id:OperationId::generate(),wakeup_id:first.definition.wakeup_id.clone() }).await?)?;
        if resumed.state != collaboration_protocol::WakeState::Active || resumed.definition.anchor_at != first.definition.anchor_at { return Err("resume changed original anchor".into()); }
        let cancelled = central_mutation(client.cancel_wakeup(collaboration_protocol::WakeMutationRequest { operation_id:OperationId::generate(),wakeup_id:first.definition.wakeup_id }).await?)?;
        if cancelled.state != collaboration_protocol::WakeState::Cancelled { return Err("cancel did not reach central".into()); }
        let invalid: WakeSendRequest = serde_json::from_value(json!({
            "operationId":OperationId::generate(),"message":current.definition.message,"timing":{"kind":"cron","expression":"*/5 * * * *","timezone":"invalid-zone"},
            "requestedBy":{"kind":"owner"},"replace":false,"expiry":{"kind":"after","seconds":7200}
        }))?;
        if !matches!(client.send_wakeup(invalid).await,
            Err(collaboration_client::WakeClientError::Catalog(collaboration_client::CentralCatalogFailure::Refused { error, .. }))
                if matches!(*error,collaboration_protocol::AutomationRefusal::Validation { error:collaboration_protocol::AutomationValidationError::InvalidTimezone })) {
            return Err("invalid timezone lacked central's typed validation guidance".into());
        }
        Ok(())
    }.await;
    let stopped = fixture.close().await;
    proof?;
    stopped
}

#[tokio::test]
async fn local_delivery_attempts_and_cancellation_preserve_original_native_evidence() -> ProofResult
{
    let fixture = CatalogFixture::open(|transport| transport, true).await?;
    let store = fixture.store.as_ref().ok_or("local storage missing")?;
    let client = fixture.api.client("legacy-delivery-history").await?;
    let target = json!({"endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},"sessionId":"fixture-only-thread"});
    let message: collaboration_protocol::SavedMessage = serde_json::from_value(
        json!({"target":target,"content":{"kind":"agent","sender":target,"text":"Check status"},"delivery":"queue","generationGuard":null}),
    )?;
    let past = (chrono::Utc::now() - chrono::Duration::seconds(1))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let immediate: WakeSendRequest = serde_json::from_value(
        json!({"operationId":OperationId::generate(),"message":message,"timing":{"kind":"at","at":past},"requestedBy":{"kind":"owner"},"replace":false,"expiry":{"kind":"none"}}),
    )?;
    let immediate_id = legacy::seed(store, immediate).await?;
    let immediate = legacy::snapshot(
        client
            .read_wakeup(WakeShowRequest {
                wakeup_id: immediate_id,
            })
            .await?,
    )?;
    let delivery = match store
        .lock()
        .await
        .evaluate_wakeup::<collaboration_protocol::SavedMessage>(
            &immediate.definition.wakeup_id,
            chrono::Utc::now().timestamp_millis(),
            wake_push_test_support::build_test_wake_push_draft,
        )
        .await?
    {
        automation_storage::WakeEvaluation::Fired { delivery_id, .. } => delivery_id,
        _ => return Err("immediate fixture wake did not fire".into()),
    };
    let empty = client
        .read_delivery_attempts(collaboration_protocol::DeliveryAttemptsRequest {
            delivery_id: delivery.clone(),
            cursor: None,
            limit: 50.try_into()?,
        })
        .await?;
    if !empty.records.is_empty() || empty.coverage.latest_attempt_included {
        return Err("empty attempt history invented submission evidence".into());
    }
    let claimed = store.lock().await.claim_delivery::<collaboration_protocol::SessionRef,collaboration_protocol::MessageContent,collaboration_protocol::CodexGeneration>(&delivery, chrono::Utc::now().timestamp_millis()).await?.ok_or("fixture claim missing")?;
    let attempts = client
        .read_delivery_attempts(collaboration_protocol::DeliveryAttemptsRequest {
            delivery_id: delivery.clone(),
            cursor: None,
            limit: 50.try_into()?,
        })
        .await?;
    let [attempt] = attempts.records.as_slice() else {
        return Err("expected one admitted delivery attempt".into());
    };
    if attempt.attempt_id != claimed.attempt_id
        || !attempts.coverage.latest_attempt_included
        || !matches!(
            attempt.evidence,
            collaboration_protocol::DeliveryEvidence::Dispatching { effects: None, .. }
        )
    {
        return Err("claimed attempt falsely reported a client dispatch".into());
    }
    // A scripted native acceptance exercises receipt projection through the real SDK.
    let generation = json!({"serviceEpoch":"00000000-0000-4000-8000-000000000001","generation":1});
    let dispatching: agent_automation::NativeEffectEvidence<
        collaboration_protocol::SessionRef,
        collaboration_protocol::CodexGeneration,
    > = serde_json::from_value(json!({
        "target":target,"generation":generation,"clientUserMessageId":claimed.attempt_id,
        "nativeTurnId":null,"nativeSubmissionId":null,
        "allocation":"notRequested","resume":"notRequested","submission":"dispatching",
        "cessation":"notApplicable"
    }))?;
    store
        .lock()
        .await
        .prepare_delivery(automation_storage::DeliveryPreparation {
            delivery_id: delivery.clone(),
            attempt_id: claimed.attempt_id.clone(),
            effects: dispatching.into(),
        })
        .await?;
    let dispatched = client
        .read_delivery_attempts(collaboration_protocol::DeliveryAttemptsRequest {
            delivery_id: delivery.clone(),
            cursor: None,
            limit: 50.try_into()?,
        })
        .await?;
    if !matches!(dispatched.records.as_slice(), [record] if matches!(record.evidence, collaboration_protocol::DeliveryEvidence::Dispatching { effects: Some(_), .. }))
    {
        return Err("first evidence write did not expose native dispatch".into());
    }
    let receipt: collaboration_protocol::NativeSendReceipt = serde_json::from_value(json!({
        "target":target,"generation":generation,"inputKind":"agent",
        "representation":"declaredAgentText","clientUserMessageId":claimed.attempt_id,
        "resumeEffect":"notRequested","acceptance":{"kind":"queueAccepted",
        "submissionId":"accepted-fixture-submission"}
    }))?;
    let effects: agent_automation::NativeEffectEvidence<
        collaboration_protocol::SessionRef,
        collaboration_protocol::CodexGeneration,
    > = serde_json::from_value(json!({
        "target":target,"generation":generation,"clientUserMessageId":claimed.attempt_id,
        "nativeTurnId":null,"nativeSubmissionId":"accepted-fixture-submission",
        "allocation":"notRequested","resume":"notRequested","submission":"accepted",
        "cessation":"notApplicable"
    }))?;
    store
        .lock()
        .await
        .complete_delivery(automation_storage::DeliveryCompletion {
            delivery_id: delivery.clone(),
            attempt_id: claimed.attempt_id,
            effects: Some(effects.into()),
            result: automation_storage::DeliveryResult::Accepted {
                effect: agent_automation::AcceptedDeliveryEffect::Queued,
                receipt: receipt.clone(),
            },
            now_ms: chrono::Utc::now().timestamp_millis(),
        })
        .await?;
    let cancellation_request = collaboration_protocol::WakeMutationRequest {
        operation_id: OperationId::generate(),
        wakeup_id: immediate.definition.wakeup_id,
    };
    let cancelled = client.cancel_wakeup(cancellation_request.clone()).await?;
    let collaboration_client::WakeMutationResponse::Local(cancelled) = cancelled else {
        return Err("legacy cancellation lost local custody".into());
    };
    let [retained] = cancelled.dispatched_deliveries.as_slice() else {
        return Err("SDK cancellation omitted previously accepted delivery".into());
    };
    if retained.delivery_id != delivery || cancelled.wakeup.pending_delivery_id.is_some() {
        return Err("SDK cancellation lost cleared-pointer delivery identity".into());
    }
    let evidence = serde_json::to_value(&retained.evidence)?;
    let expected: collaboration_protocol::DeliveryReceipt = receipt.into();
    if evidence.get("receipt") != Some(&serde_json::to_value(expected)?) {
        return Err("SDK cancellation changed old stored native receipt meaning".into());
    }
    let replayed = client.cancel_wakeup(cancellation_request).await?;
    let collaboration_client::WakeMutationResponse::Local(replayed) = replayed else {
        return Err("legacy cancellation replay changed owner".into());
    };
    if serde_json::to_value(&cancelled)? != serde_json::to_value(replayed)? {
        return Err("SDK cancellation replay changed evidence".into());
    }
    fixture.close().await
}

fn central_create(
    response: collaboration_client::WakeCreateResponse,
) -> ProofResult<collaboration_protocol::CentralWakeSnapshot> {
    match response {
        collaboration_client::WakeCreateResponse::Central { value } => Ok(value),
        collaboration_client::WakeCreateResponse::Legacy { .. } => {
            Err("new create did not return central ownership".into())
        }
    }
}
fn central_read(
    response: collaboration_client::WakeReadResponse,
) -> ProofResult<collaboration_protocol::CentralWakeSnapshot> {
    match response {
        collaboration_client::WakeReadResponse::Central { value } => Ok(value),
        _ => Err("central read changed owner".into()),
    }
}
fn central_mutation(
    response: collaboration_client::WakeMutationResponse,
) -> ProofResult<collaboration_protocol::CentralWakeSnapshot> {
    let collaboration_client::WakeMutationResponse::Central(result) = response else {
        return Err("central control changed owner".into());
    };
    if !result.already_sent.is_empty() {
        return Err("future Wake control fabricated sent Firings".into());
    }
    match result.automation {
        collaboration_client::CatalogAutomationResult::Wake(value) => Ok(value),
        _ => Err("central control changed automation kind".into()),
    }
}
