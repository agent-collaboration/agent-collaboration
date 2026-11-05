//! The actual settlement producer observes the one committed Fleet view.
use super::{composed_fleet_fixture::*, fleet_claim_adapter_fixture::*};
use collaboration_central_conformance::{
    DispatchFaultObservation, ObjectReplyFaultObservation, RuntimeControl,
};
use durable_steps::{
    AtomicStore, OperationId, StepName, StoreError,
    storage::{CallReach, JournalStorage, OperationExecution, StepState},
};
use serde_json::{Value, json};

fn require_call_identity(call: &Value, operation: OperationId) -> TestResult {
    if call.pointer("/identity/operation") != Some(&json!(operation.as_uuid()))
        || call.pointer("/identity/step") != Some(&json!("settle"))
    {
        return Err("settlement producer changed the original durable CallId".into());
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A real successor snapshot must be receipted once despite an actual lost local answer and owning-store reopen."
)]
async fn granted_settlement_reads_the_public_snapshot_and_replays_its_original_call_after_answer_loss()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = prepare_fleet(root.path(), true).await?;
    let before = fixture.lease_and_notices().await?;
    assert_eq!(before.0.pointer("/revision/epoch"), Some(&json!(2)));
    assert_eq!(before.0.pointer("/grant/holder"), Some(&json!(B)));
    let operation = admit_settlement(&fixture, 1).await?;
    hold_next_settlement(&fixture).await?;
    let controller = async {
        let call = held_settlement(&fixture).await?;
        require_call_identity(&call, operation)?;
        assert_eq!(
            call.pointer("/request/projection/snapshot"),
            Some(&before.0)
        );
        assert_eq!(
            call.pointer("/request/projection/holder"),
            Some(&json!(SERVICE_B))
        );
        fixture
            .io
            .local_call_faults
            .control(RuntimeControl::DropHeldDispatchReply {
                key: settlement_key(),
            })
            .await?;
        resume_settlement(&fixture).await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(call)
    };
    let (lost, first_call) = Box::pin(tokio::time::timeout(WAIT_BOUND, async {
        tokio::try_join!(drive_fleet(&fixture), controller)
    }))
    .await??;
    assert!(lost.retried.contains(&operation));
    let observed = fixture.io.local_call_faults.observe()?;
    assert!(
        matches!(observed.object_reply, ObjectReplyFaultObservation::Dropped {request,..} if serde_json::from_slice::<Value>(&request)? == first_call)
    );
    let committed = read_receipt(&fixture, operation).await?;
    assert_eq!(committed.count, 1);
    assert_eq!(committed.operation, operation.as_uuid().to_string());
    assert_eq!(committed.step, "settle");
    assert_eq!(
        durable_steps::Uuid::parse_str(&committed.id)?.get_version_num(),
        7
    );
    assert_eq!((committed.created_at, committed.updated_at), (NOW, NOW));
    assert_eq!(committed.result, json!({"kind":"duplicate"}));
    assert_eq!(fixture.lease_and_notices().await?, before);
    fixture.io.advance(60_000);
    hold_next_settlement(&fixture).await?;
    let retry = async {
        let call = held_settlement(&fixture).await?;
        assert_eq!(call, first_call);
        resume_settlement(&fixture).await
    };
    let (finished, ()) = Box::pin(tokio::time::timeout(WAIT_BOUND, async {
        tokio::try_join!(drive_fleet(&fixture), retry)
    }))
    .await??;
    assert!(finished.completed.contains(&operation));
    assert_eq!(read_receipt(&fixture, operation).await?, committed);
    assert_eq!(fixture.lease_and_notices().await?, before);
    fixture.close().await?;
    let reopened = Fixture::open(root.path(), NOW + 60_000).await?;
    assert_eq!(read_receipt(&reopened, operation).await?, committed);
    assert_eq!(reopened.lease_and_notices().await?, before);
    let record = reopened
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                scope
                    .operation(operation)
                    .await?
                    .ok_or(StoreError::MissingOperation { id: operation })
            })
        })
        .await?;
    assert!(matches!(
        record.execution,
        OperationExecution::Completed { .. }
    ));
    reopened.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Fresh duplicate settlement must create one receipt without writing the already committed hook view."
)]
async fn duplicate_projection_is_recorded_by_a_new_claim_call_and_the_adapter_completes()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = prepare_fleet(root.path(), false).await?;
    let before = fixture.lease_and_notices().await?;
    assert_eq!(claim_counts(&fixture).await?, (0, 0));
    let operation = admit_settlement(&fixture, 1).await?;
    assert!(drive_fleet(&fixture).await?.completed.contains(&operation));
    let receipt = read_receipt(&fixture, operation).await?;
    assert_eq!(receipt.result, json!({"kind":"duplicate"}));
    assert_eq!(
        receipt.reservation,
        json!({"kind":"settled","value":{"outcome":"granted"}})
    );
    assert_eq!(claim_counts(&fixture).await?, (1, 1));
    assert_eq!(fixture.lease_and_notices().await?, before);
    fixture.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A real renewal must commit while the older captured settlement is held, and Older must complete without a second view writer."
)]
async fn a_newer_real_publication_overtakes_the_claim_call_and_older_still_completes() -> TestResult
{
    let root = tempfile::tempdir()?;
    let fixture = prepare_fleet(root.path(), false).await?;
    let operation = admit_settlement(&fixture, 1).await?;
    hold_next_settlement(&fixture).await?;
    let controller = async {
        let captured = held_settlement(&fixture).await?;
        require_call_identity(&captured, operation)?;
        assert_eq!(
            captured.pointer("/request/projection/snapshot/revision/change"),
            Some(&json!(1))
        );
        fixture.io.advance(10_000);
        let renewed = fixture.call("session", "central.session-lease", Value::Null, json!({
            "session":{"peer":A,"sessionId":durable_steps::Uuid::now_v7()},
            "request":{"kind":"renew","value":{"attempt":durable_steps::Uuid::now_v7(),"database":PROJECT,"epoch":1}}
        })).await?;
        assert!(matches!(
            serde_json::from_value::<fleet_sync_protocol::LeaseAnswer>(renewed)?,
            fleet_sync_protocol::LeaseAnswer::Renew(fleet_sync_protocol::RenewAnswer::Renewed {
                replayed: false,
                ..
            })
        ));
        let newer = fixture.lease_and_notices().await?;
        assert_eq!(newer.0.pointer("/revision/change"), Some(&json!(2)));
        assert_eq!(claim_counts(&fixture).await?, (0, 0));
        resume_settlement(&fixture).await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(newer)
    };
    let (completed, newer) = Box::pin(tokio::time::timeout(WAIT_BOUND, async {
        tokio::try_join!(drive_fleet(&fixture), controller)
    }))
    .await??;
    assert!(completed.completed.contains(&operation));
    assert_eq!(
        read_receipt(&fixture, operation).await?.result,
        json!({"kind":"older","value":{"stored":{"epoch":1,"incarnation":2,"change":2}}})
    );
    assert_eq!(claim_counts(&fixture).await?, (1, 1));
    assert_eq!(fixture.lease_and_notices().await?, newer);
    fixture.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A grant newer than the genuine snapshot must keep the exact call pending without reaching claim settlement."
)]
async fn a_snapshot_behind_the_grant_keeps_the_same_call_pending_without_settling_fleet()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = prepare_fleet(root.path(), false).await?;
    let before = fixture.lease_and_notices().await?;
    let operation = admit_settlement(&fixture, 2).await?;
    hold_next_settlement(&fixture).await?;
    let mut pending = None;
    for _ in 0..2 {
        assert!(drive_fleet(&fixture).await?.retried.contains(&operation));
        let step = fixture
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    scope
                        .step(operation, 0)
                        .await?
                        .ok_or_else(|| StoreError::backend("settlement step absent"))
                })
            })
            .await?;
        assert_eq!(step.operation, operation);
        assert_eq!(step.name, StepName::new("settle")?);
        assert!(
            matches!(&step.state, StepState::CallPending {attempt,..} if attempt.reach == CallReach::MayHaveReached && attempt.retry_reason.is_none())
        );
        if let Some(previous) = &pending {
            assert_eq!(&step, previous);
        }
        pending = Some(step);
        assert_eq!(claim_counts(&fixture).await?, (0, 0));
        assert_eq!(fixture.lease_and_notices().await?, before);
        assert!(matches!(
            fixture
                .io
                .local_call_faults
                .observe()?
                .dispatch(&settlement_key()),
            DispatchFaultObservation::Armed { .. }
        ));
        fixture.io.advance(60_000);
    }
    fixture.close().await
}
