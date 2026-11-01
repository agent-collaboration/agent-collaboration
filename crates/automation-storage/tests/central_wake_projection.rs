//! A replacement definition does not cancel a central Firing already sent to this machine.
use agent_automation::{DeliveryStatus, NativeEffectEvidence, SubmissionEffect};
use automation_storage::{
    AutomationStore, CentralFiringRequest, CentralLocalWork, DeliveryCompletion,
    DeliveryPreparation, DeliveryResult,
};
use collaboration_protocol::{
    AutomationContent, BatchId, CodexGeneration, FiringEntry, MessageContent, SessionRef,
};
use serde_json::json;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn sent_wake() -> TestResult<CentralFiringRequest> {
    let firing: FiringEntry = serde_json::from_value(json!({
        "firingId":uuid::Uuid::now_v7().to_string(),"automationId":uuid::Uuid::now_v7().to_string(),
        "dueAt":300000,"revision":uuid::Uuid::now_v7().to_string(),
        "content":{"contentId":uuid::Uuid::now_v7().to_string(),"revisionId":uuid::Uuid::now_v7().to_string(),"digest":"a".repeat(64)},
        "work":{"kind":"wake","createdBy":{"kind":"owner"},"timing":{"kind":"at","at":"1970-01-01T00:05:00Z"},
            "anchorAt":"1970-01-01T00:00:00Z","createdAt":"1970-01-01T00:00:00Z",
            "target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000002","endpointId":"codex-local"},"sessionId":"same-recipient"},
            "origin":{"kind":"router"},"delivery":"auto","generationGuard":null,"expiresAt":"1970-01-01T00:20:00Z"}
    }))?;
    Ok(CentralFiringRequest {
        batch_id: BatchId::generate(),
        content: AutomationContent {
            reference: firing.content.clone(),
            text: "Already sent wake".to_owned().try_into()?,
        },
        firing,
        now_ms: 400_000,
    })
}

#[tokio::test]
async fn replacement_keeps_the_sent_pending_delivery_claimable_after_reopen() -> TestResult {
    replacement_preserves_delivery(DeliveryStatus::Pending).await
}

#[tokio::test]
async fn replacement_keeps_uncertain_attempt_evidence_without_replaying_it() -> TestResult {
    replacement_preserves_delivery(DeliveryStatus::Uncertain).await
}

async fn replacement_preserves_delivery(status: DeliveryStatus) -> TestResult {
    // Arrange: W1 was sent before the central replacement cutoff.
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    let mut store = AutomationStore::open(&path).await?;
    let first_request = sent_wake()?;
    let receipt = store.admit_central_firing(&first_request).await?;
    let CentralLocalWork::Wake { delivery_id } = receipt.local_work else {
        return Err("first sent wake did not create a delivery".into());
    };
    if status == DeliveryStatus::Uncertain {
        let claim = store
            .claim_delivery::<SessionRef, MessageContent, CodexGeneration>(&delivery_id, 400_000)
            .await?
            .ok_or("first delivery is not claimable")?;
        let mut effects: NativeEffectEvidence<SessionRef, CodexGeneration> =
            serde_json::from_value(json!({
                "target":claim.target,"generation":{"serviceEpoch":"00000000-0000-4000-8000-000000000001","generation":1},
                "clientUserMessageId":null,"nativeTurnId":null,"nativeSubmissionId":null,
                "allocation":"notRequested","resume":"notRequested","submission":"dispatching","cessation":"notApplicable"
            }))?;
        if !store
            .prepare_delivery(DeliveryPreparation {
                delivery_id: delivery_id.clone(),
                attempt_id: claim.attempt_id.clone(),
                effects: effects.clone().into(),
            })
            .await?
        {
            return Err("first attempt did not retain its selected route".into());
        }
        effects.submission = SubmissionEffect::Unknown;
        if !store
            .complete_delivery(DeliveryCompletion::<_, _, serde_json::Value> {
                delivery_id: delivery_id.clone(),
                attempt_id: claim.attempt_id,
                effects: Some(effects.into()),
                result: DeliveryResult::Unknown {
                    reason: "Native answer was lost".to_owned(),
                    receipt: None,
                },
                now_ms: 400_001,
            })
            .await?
        {
            return Err("first attempt did not retain its unknown outcome".into());
        }
    }
    let before = store
        .read_delivery::<SessionRef, CodexGeneration, serde_json::Value>(&delivery_id)
        .await?;
    if before.status != status {
        return Err("fixture did not reach the requested delivery state".into());
    }

    // Act: W2's independent, already-sent Firing arrives for the same target.
    let second = store.admit_central_firing(&sent_wake()?).await?;
    let CentralLocalWork::Wake {
        delivery_id: second_delivery,
    } = second.local_work
    else {
        return Err("replacement sent wake did not create a delivery".into());
    };
    store.close().await?;
    let mut store = AutomationStore::open(&path).await?;

    // Assert: W1 retains its exact original delivery and attempt bytes across replacement/reopen.
    let after = store
        .read_delivery::<SessionRef, CodexGeneration, serde_json::Value>(&delivery_id)
        .await?;
    if serde_json::to_value(&before)? != serde_json::to_value(&after)? {
        return Err("replacement changed the already-sent delivery or its evidence".into());
    }
    let replay = store.admit_central_firing(&first_request).await?;
    if !matches!(replay.local_work, CentralLocalWork::Wake { delivery_id: same } if same == delivery_id)
    {
        return Err("replayed first Firing created another delivery".into());
    }
    let first_claim = store
        .claim_delivery::<SessionRef, MessageContent, CodexGeneration>(&delivery_id, 400_002)
        .await?;
    if first_claim.is_some() != (status == DeliveryStatus::Pending) {
        return Err("pending delivery was discarded or uncertain delivery was replayed".into());
    }
    if store
        .claim_delivery::<SessionRef, MessageContent, CodexGeneration>(&second_delivery, 400_002)
        .await?
        .is_none()
    {
        return Err("first sent wake blocked its replacement delivery".into());
    }
    store.close().await?;
    Ok(())
}
