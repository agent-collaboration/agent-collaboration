//! Provider schedule outcomes and retry identity, including push retention expiry.
use super::route_tests::{
    FakeSettlementPlan, FakeSubmissionPlan, TestResult, apply_push_handoff, provider_worker_fixture,
};
use agent_automation::{RouteEffectEvidence, RunPhase};
use automation_storage::StorageError;
use collaboration_protocol::{
    CodexGeneration, DeliveryNextAction, DeliveryOutcome, DeliveryRejection,
    DeliveryRejectionReason, EndpointRef, PushDeliveryState, PushHeaderFacts, PushKind, SessionRef,
};
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn fake_provider_submission_and_settlement_variants_preserve_run_state() -> TestResult<()> {
    let cases = [
        (
            FakeSubmissionPlan::Reject,
            FakeSettlementPlan::Pending,
            RunPhase::Preparing,
            false,
        ),
        (
            FakeSubmissionPlan::Unknown,
            FakeSettlementPlan::Pending,
            RunPhase::Uncertain,
            false,
        ),
        (
            FakeSubmissionPlan::Accept,
            FakeSettlementPlan::Failed,
            RunPhase::Finished,
            false,
        ),
        (
            FakeSubmissionPlan::Accept,
            FakeSettlementPlan::Interrupted,
            RunPhase::Finished,
            false,
        ),
        (
            FakeSubmissionPlan::Accept,
            FakeSettlementPlan::Pending,
            RunPhase::Executing,
            false,
        ),
        (
            FakeSubmissionPlan::Accept,
            FakeSettlementPlan::WrittenWithoutCompletion,
            RunPhase::Executing,
            true,
        ),
    ];
    for (submission, settlement, expected_phase, expected_error) in cases {
        let (root, store, worker, run_id, observed_push_ids) =
            provider_worker_fixture(submission, settlement).await?;
        Box::pin(worker.step(run_id.clone())).await?;
        apply_push_handoff(&worker).await?;
        Box::pin(worker.step(run_id.clone())).await?;
        apply_push_handoff(&worker).await?;
        let busy = store
            .lock()
            .await
            .read_run::<SessionRef, EndpointRef, CodexGeneration, crate::stored_run_receipt::StoredRunReceipt>(&run_id)
            .await?;
        if busy.phase != RunPhase::Preparing || busy.evidence.timing.is_some() {
            return Err("not-started-busy consumed a dispatch budget".into());
        }
        Box::pin(worker.step(run_id.clone())).await?;
        apply_push_handoff(&worker).await?;
        if matches!(submission, FakeSubmissionPlan::Accept) {
            let observed = Box::pin(worker.step(run_id.clone())).await;
            if expected_error {
                if !matches!(observed, Err(StorageError::InvalidRecord)) {
                    return Err("provider write-only settlement was not rejected".into());
                }
            } else {
                observed?;
                apply_push_handoff(&worker).await?;
            }
        }
        let settled = store
            .lock()
            .await
            .read_run::<SessionRef, EndpointRef, CodexGeneration, crate::stored_run_receipt::StoredRunReceipt>(&run_id)
            .await?;
        if settled.phase != expected_phase {
            return Err(format!(
                "provider route phase mismatch: expected {expected_phase:?}, got {:?}",
                settled.phase
            )
            .into());
        }
        let observed_push_ids_snapshot = observed_push_ids
            .lock()
            .map_err(|_| "scheduled push id observation lock poisoned")?
            .clone();
        if observed_push_ids_snapshot.len() != 2
            || observed_push_ids_snapshot[0] != observed_push_ids_snapshot[1]
        {
            return Err("schedule retry did not reuse the same push id".into());
        }
        let push = worker
            .interactions
            .get_push_record(&observed_push_ids_snapshot[0])
            .await?
            .ok_or("scheduled push record missing")?;
        let (expected_push_state, expected_push_outcome) = match submission {
            FakeSubmissionPlan::Reject => (
                PushDeliveryState::Pending,
                Some(DeliveryOutcome::Rejected(DeliveryRejection {
                    reason: DeliveryRejectionReason::Busy,
                    next_action: DeliveryNextAction::RetryLater,
                    client_code: None,
                    detail: Some("fixture rejection".into()),
                    claims: None,
                    native_failure: None,
                })),
            ),
            FakeSubmissionPlan::Unknown => (
                PushDeliveryState::OutcomeUnknown,
                Some(DeliveryOutcome::Unknown),
            ),
            FakeSubmissionPlan::Accept | FakeSubmissionPlan::RejectOnceThenAccept => {
                (PushDeliveryState::Delivered, Some(DeliveryOutcome::Started))
            }
            FakeSubmissionPlan::CrashAfterDispatchIntent => {
                return Err("crash fixture must stop before submission settlement".into());
            }
        };
        if push.delivery_state != expected_push_state
            || push
                .last_outcome
                .as_ref()
                .map(|receipt| receipt.outcome.clone())
                != expected_push_outcome
        {
            return Err("scheduled push outcome did not match the run submission".into());
        }
        if matches!(submission, FakeSubmissionPlan::Unknown) {
            Box::pin(worker.step(run_id.clone())).await?;
            apply_push_handoff(&worker).await?;
            let observed_push_ids_after_observation = observed_push_ids
                .lock()
                .map_err(|_| "scheduled push id observation lock poisoned")?
                .clone();
            if observed_push_ids_after_observation.len() != 2 {
                return Err("unknown provider submission was automatically redispatched".into());
            }
            let uncertain_run = store
                .lock()
                .await
                .read_run::<
                    SessionRef,
                    EndpointRef,
                    CodexGeneration,
                    crate::stored_run_receipt::StoredRunReceipt,
                >(&run_id)
                .await?;
            if uncertain_run.phase != RunPhase::Uncertain {
                return Err("unknown provider submission left the uncertain phase".into());
            }
            let uncertain_push = worker
                .interactions
                .get_push_record(&observed_push_ids_after_observation[0])
                .await?
                .ok_or("unknown schedule push missing")?;
            if uncertain_push.delivery_state != PushDeliveryState::OutcomeUnknown {
                return Err("unknown provider submission changed push state on observation".into());
            }
        }
        let Some(RouteEffectEvidence::ProviderAcp(provider_effect)) =
            settled.evidence.route.as_ref()
        else {
            return Err("scheduled run lost its selected provider evidence".into());
        };
        let PushHeaderFacts::ScheduleRun {
            schedule_id: push_schedule_id,
            run_id: push_run_id,
        } = &push.header_facts
        else {
            return Err("scheduled push lost its typed schedule origin".into());
        };
        if push.push_id != observed_push_ids_snapshot[0]
            || push.target != provider_effect.target
            || push_schedule_id != &settled.schedule_id
            || push_run_id != &settled.run_id
        {
            return Err("scheduled push identity, target, or route evidence diverged".into());
        }
        if matches!(submission, FakeSubmissionPlan::Reject) && settled.evidence.timing.is_some() {
            return Err("known rejection retained an execution budget".into());
        }
        match (submission, settlement) {
            (FakeSubmissionPlan::Accept, FakeSettlementPlan::Failed)
                if !matches!(
                    settled.worker_outcome,
                    Some(agent_automation::WorkerOutcome::Failed { .. })
                ) =>
            {
                return Err("failed settlement lost worker outcome".into());
            }
            (FakeSubmissionPlan::Accept, FakeSettlementPlan::Interrupted)
                if !matches!(
                    settled.worker_outcome,
                    Some(agent_automation::WorkerOutcome::Interrupted { .. })
                ) =>
            {
                return Err("interrupted settlement lost worker outcome".into());
            }
            _ => {}
        }
        let interactions = Arc::clone(&worker.interactions);
        drop(worker);
        Arc::try_unwrap(interactions)
            .map_err(|_| "schedule fixture interaction owner remained shared")?
            .close()
            .await?;
        let store = Arc::try_unwrap(store).map_err(|_| "store still referenced")?;
        store.into_inner().close().await?;
        for entry in std::fs::read_dir(&root)? {
            std::fs::remove_file(entry?.path())?;
        }
        std::fs::remove_dir(root)?;
    }
    Ok(())
}

#[tokio::test]
async fn shared_restore_preserves_attempted_schedule_push_for_worker_settlement() -> TestResult<()>
{
    let (root, store, worker, run_id, observed_push_ids) = provider_worker_fixture(
        FakeSubmissionPlan::CrashAfterDispatchIntent,
        FakeSettlementPlan::Pending,
    )
    .await?;
    Box::pin(worker.step(run_id.clone())).await?;
    apply_push_handoff(&worker).await?;
    Box::pin(worker.step(run_id.clone())).await?;
    apply_push_handoff(&worker).await?;
    if !matches!(
        Box::pin(worker.step(run_id.clone())).await,
        Err(StorageError::InvalidRecord)
    ) {
        return Err("fixture did not stop after persisting schedule dispatch intent".into());
    }

    let push_ids = observed_push_ids
        .lock()
        .map_err(|_| "schedule push id observation lock poisoned")?
        .clone();
    if push_ids.len() != 2 || push_ids[0] != push_ids[1] {
        return Err("interrupted run did not retain one schedule push identity".into());
    }
    let interrupted_push = worker
        .interactions
        .get_push_record(&push_ids[0])
        .await?
        .ok_or("interrupted schedule push missing")?;
    let interrupted_run = store
        .lock()
        .await
        .read_run::<
            SessionRef,
            EndpointRef,
            CodexGeneration,
            crate::stored_run_receipt::StoredRunReceipt,
        >(&run_id)
        .await?;
    if interrupted_push.delivery_state != PushDeliveryState::Attempted
        || interrupted_run.phase != RunPhase::Preparing
        || interrupted_run.evidence.timing.is_none()
    {
        return Err("fixture did not retain attempted push and dispatch evidence".into());
    }

    let settled_by_shared_restore = worker
        .interactions
        .settle_interrupted_pushes(chrono::Utc::now())
        .await?;
    if settled_by_shared_restore != 0 {
        return Err("shared restore settled a ScheduleRun-owned attempt".into());
    }
    let still_attempted = worker
        .interactions
        .get_push_record(&push_ids[0])
        .await?
        .ok_or("schedule push missing after shared restore")?;
    if still_attempted.delivery_state != PushDeliveryState::Attempted {
        return Err("shared restore removed the attempt from schedule recovery".into());
    }

    Box::pin(worker.step(run_id.clone())).await?;

    apply_push_handoff(&worker).await?;
    let recovered_run = store
        .lock()
        .await
        .read_run::<
            SessionRef,
            EndpointRef,
            CodexGeneration,
            crate::stored_run_receipt::StoredRunReceipt,
        >(&run_id)
        .await?;
    let recovered_push = worker
        .interactions
        .get_push_record(&push_ids[0])
        .await?
        .ok_or("schedule push missing after owner recovery")?;
    if recovered_run.phase != RunPhase::Uncertain
        || recovered_push.delivery_state != PushDeliveryState::OutcomeUnknown
        || recovered_push
            .last_outcome
            .as_ref()
            .map(|receipt| &receipt.outcome)
            != Some(&DeliveryOutcome::Unknown)
    {
        return Err("schedule worker did not settle the original interrupted attempt".into());
    }
    if observed_push_ids
        .lock()
        .map_err(|_| "schedule push id observation lock poisoned")?
        .len()
        != 2
    {
        return Err("schedule worker replayed the interrupted start".into());
    }

    let interactions = Arc::clone(&worker.interactions);

    drop(worker);

    Arc::try_unwrap(interactions)
        .map_err(|_| "schedule fixture interaction owner remained shared")?
        .close()
        .await?;
    let unique_store = Arc::try_unwrap(store).map_err(|_| "store still referenced")?;
    unique_store.into_inner().close().await?;
    for entry in std::fs::read_dir(&root)? {
        std::fs::remove_file(entry?.path())?;
    }
    std::fs::remove_dir(root)?;
    Ok(())
}

#[tokio::test]
async fn known_rejected_provider_submission_retries_on_the_same_schedule_push_id() -> TestResult<()>
{
    let (root, store, worker, run_id, observed_push_ids) = provider_worker_fixture(
        FakeSubmissionPlan::RejectOnceThenAccept,
        FakeSettlementPlan::Pending,
    )
    .await?;
    Box::pin(worker.step(run_id.clone())).await?;
    apply_push_handoff(&worker).await?;
    Box::pin(worker.step(run_id.clone())).await?;
    apply_push_handoff(&worker).await?;
    Box::pin(worker.step(run_id.clone())).await?;
    apply_push_handoff(&worker).await?;

    let first_attempt_ids = observed_push_ids
        .lock()
        .map_err(|_| "scheduled push id observation lock poisoned")?
        .clone();
    if first_attempt_ids.len() != 2 || first_attempt_ids[0] != first_attempt_ids[1] {
        return Err("known rejection did not retain its original schedule push id".into());
    }
    let rejected_push = worker
        .interactions
        .get_push_record(&first_attempt_ids[0])
        .await?
        .ok_or("rejected schedule push missing")?;
    if rejected_push.delivery_state != PushDeliveryState::Pending
        || !matches!(
            rejected_push
                .last_outcome
                .as_ref()
                .map(|receipt| &receipt.outcome),
            Some(DeliveryOutcome::Rejected(_))
        )
        || rejected_push.settled_at.is_none()
    {
        return Err("known rejection was not retained on a retryable pending push".into());
    }

    Box::pin(worker.step(run_id.clone())).await?;

    apply_push_handoff(&worker).await?;
    let accepted_run = store
        .lock()
        .await
        .read_run::<SessionRef, EndpointRef, CodexGeneration, crate::stored_run_receipt::StoredRunReceipt>(&run_id)
        .await?;
    if accepted_run.phase != RunPhase::Executing {
        return Err("known rejection retry did not advance the Run after acceptance".into());
    }
    let all_attempt_ids = observed_push_ids
        .lock()
        .map_err(|_| "scheduled push id observation lock poisoned")?
        .clone();
    if all_attempt_ids.len() != 3
        || all_attempt_ids
            .iter()
            .any(|push_id| push_id != &first_attempt_ids[0])
    {
        return Err("schedule retry created or dispatched a second push id".into());
    }
    let delivered_push = worker
        .interactions
        .get_push_record(&first_attempt_ids[0])
        .await?
        .ok_or("delivered schedule push missing")?;
    if delivered_push.delivery_state != PushDeliveryState::Delivered
        || !matches!(
            delivered_push
                .last_outcome
                .as_ref()
                .map(|receipt| &receipt.outcome),
            Some(DeliveryOutcome::Started)
        )
    {
        return Err("schedule push did not settle delivered after its retry".into());
    }
    let interactions = Arc::clone(&worker.interactions);
    drop(worker);
    Arc::try_unwrap(interactions)
        .map_err(|_| "schedule fixture interaction owner remained shared")?
        .close()
        .await?;
    let store = Arc::try_unwrap(store).map_err(|_| "store still referenced")?;
    store.into_inner().close().await?;
    for entry in std::fs::read_dir(&root)? {
        std::fs::remove_file(entry?.path())?;
    }
    std::fs::remove_dir(root)?;
    Ok(())
}

#[path = "scheduled_push_retention_tests.rs"]
mod retention_tests;
