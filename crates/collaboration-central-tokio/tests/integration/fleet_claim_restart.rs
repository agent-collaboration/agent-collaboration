//! Committed settlement work resumes through the actual local Fleet dispatcher.
use super::{composed_fleet_fixture::*, fleet_claim_adapter_fixture::*};
use durable_steps::{
    AtomicStore, CallTarget, StoreError,
    storage::{JournalStorage, OperationExecution},
};
use fleet_sync_protocol::{SettleOutcome, SettlementRecorded};
use serde_json::{Value, json};

#[expect(
    clippy::panic_in_result_fn,
    reason = "Owning-store reopen must resume the original task once, retain its real result, and preserve the committed lease view."
)]
#[tokio::test]
async fn restart_drives_a_committed_registry_task_and_keeps_its_result() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = prepare_fleet(root.path(), false).await?;
    let before = fixture.lease_and_notices().await?;
    let operation = admit_settlement_outcome(&fixture, SettleOutcome::Cancelled).await?;
    let pending = fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                let record = scope
                    .operation(operation)
                    .await?
                    .ok_or(StoreError::MissingOperation { id: operation })?;
                let step = scope.step(operation, 0).await?;
                Ok((record, step))
            })
        })
        .await?;
    let OperationExecution::Running { input, .. } = pending.0.execution else {
        return Err("settlement completed before any driver ran".into());
    };
    let expected_request: Value = serde_json::from_slice(&input)?;
    assert!(
        pending.1.is_none(),
        "no driver ran before the owning store closed"
    );
    assert_eq!(claim_counts(&fixture).await?, (0, 0));
    fixture.close().await?;

    let reopened = Fixture::open(root.path(), NOW).await?;
    assert_eq!(claim_counts(&reopened).await?, (0, 0));
    hold_next_settlement(&reopened).await?;
    let controller = async {
        let call = held_settlement_kind(&reopened, fleet_sync_lease::ClaimSettlement::KIND).await?;
        assert_eq!(call.get("source"), Some(&json!({"kind":"fleet"})));
        assert_eq!(call.get("sourceFace"), Some(&json!("registry")));
        assert_eq!(call.get("destinationFace"), Some(&json!("fleet")));
        assert_eq!(call.get("request"), Some(&expected_request));
        assert_eq!(
            call.pointer("/identity/operation"),
            Some(&json!(operation.as_uuid()))
        );
        assert_eq!(call.pointer("/identity/step"), Some(&json!("settle")));
        let request: fleet_sync_protocol::ClaimSettlement =
            serde_json::from_value(call["request"].clone())?;
        assert_eq!(request.outcome, SettleOutcome::Cancelled);
        assert!(
            matches!(request.change, fleet_sync_protocol::HolderChangeRequest::Claim {database,claimant,..} if database.to_string() == PROJECT && claimant.to_string() == A)
        );
        resume_settlement(&reopened).await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(call)
    };
    let (report, call) = Box::pin(tokio::time::timeout(WAIT_BOUND, async {
        tokio::try_join!(drive_fleet(&reopened), controller)
    }))
    .await??;
    assert_eq!(report.completed, vec![operation]);
    assert!(report.failed.is_empty());
    assert!(report.retried.is_empty());
    assert!(report.yielded.is_empty());
    assert!(report.wake_failure.is_none());
    assert_eq!(claim_counts(&reopened).await?, (1, 0));
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
    assert!(
        matches!(record.execution, OperationExecution::Completed {result,..} if result == serde_json::to_vec(&SettlementRecorded)?)
    );
    assert!(!reopened.io.wakes()?.is_empty());
    assert_eq!(
        drive_fleet(&reopened).await?,
        durable_steps::DriveReport::default()
    );
    assert_eq!(claim_counts(&reopened).await?, (1, 0));
    assert!(
        reopened
            .io
            .local_call_faults
            .observe()?
            .dispatches
            .is_empty()
    );
    reopened.close().await?;

    let completed = Fixture::open(root.path(), NOW).await?;
    let cached: Value = completed
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                let record = scope
                    .operation(operation)
                    .await?
                    .ok_or(StoreError::MissingOperation { id: operation })?;
                let OperationExecution::Completed { result, .. } = record.execution else {
                    return Err(StoreError::backend(
                        "completed settlement was lost on reopen",
                    ));
                };
                serde_json::from_slice(&result).map_err(StoreError::backend)
            })
        })
        .await?;
    assert_eq!(cached, serde_json::to_value(SettlementRecorded)?);
    assert_eq!(
        call.pointer("/identity/operation"),
        Some(&json!(operation.as_uuid()))
    );
    assert_eq!(
        drive_fleet(&completed).await?,
        durable_steps::DriveReport::default()
    );
    assert_eq!(claim_counts(&completed).await?, (1, 0));
    assert_eq!(completed.lease_and_notices().await?, before);
    completed.close().await
}
