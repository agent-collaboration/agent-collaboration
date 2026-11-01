//! Application inspection stays attached to the real runtime schedule steps.
use agent_automation::{ContinuityInput, ExecutionDestination, ScheduleDefinition};

use agent_automation::*;
use automation_storage::*;
use collaboration_contracts::*;
use collaboration_protocol::*;
use collaboration_runtime::test_support::scheduled_route_fixture::*;
use collaboration_runtime::test_support::*;
use serde_json::json;
use std::sync::{Arc, atomic::AtomicUsize};
use tokio::sync::Mutex;
#[path = "scheduled_history_retention_tests.rs"]
mod history_retention_tests;

#[tokio::test]
async fn provider_and_peer_routes_drive_run_show_without_native_turns() -> TestResult<()> {
    for kind in [FakeRouteKind::Provider, FakeRouteKind::Peer] {
        let route_name = match kind {
            FakeRouteKind::Provider => "provider",
            FakeRouteKind::Peer => "peer",
        };
        let root = std::env::temp_dir().join(format!(
            "fake-run-route-{}",
            OperationId::generate().as_str()
        ));
        std::fs::create_dir(&root)?;
        let store = Arc::new(Mutex::new(
            AutomationStore::open(&root.join("automation.sqlite")).await?,
        ));
        let service = "00000000-0000-4000-8000-000000000001";
        let interactions = Arc::new(
            interaction_storage::InteractionStore::open(root.join("interaction.sqlite")).await?,
        );
        let target: SessionRef = serde_json::from_value(json!({
            "endpoint":{"serviceId":service,"endpointId":"claude-local"},
            "sessionId":match kind { FakeRouteKind::Provider => "provider-worker", FakeRouteKind::Peer => "peer-worker" }
        }))?;
        let generation: CodexGeneration =
            serde_json::from_value(json!({"serviceEpoch":service,"generation":1}))?;
        let fake: Arc<dyn ScheduledRunExecution> = Arc::new(FakeScheduledExecution {
            kind,
            target: target.clone(),
            interactions: Arc::clone(&interactions),
            observed_push_ids: Arc::new(std::sync::Mutex::new(Vec::new())),
            generation,
            attempt_id: AttemptId::generate(),
            submissions: AtomicUsize::new(0),
            submission_plan: FakeSubmissionPlan::Accept,
            settlement_plan: FakeSettlementPlan::Completed,
        });
        let instruction = store
            .lock()
            .await
            .create_instruction(
                &OperationId::generate(),
                &InstructionText::try_from("Check job".to_owned())?,
                0,
            )
            .await?;
        let schedule = store
            .lock()
            .await
            .create_schedule(
                &ScheduleCreate::<SessionRef, EndpointRef, message_board::ProjectId> {
                    operation_id: OperationId::generate(),
                    definition: ScheduleDefinition {
                        instruction_id: instruction.instruction_id,
                        timing: TimingRule::Interval { seconds: 60 },
                        enabled: true,
                        destination: ExecutionDestination::OwnedThread {
                            target: target.clone(),
                            cwd: root.to_string_lossy().into_owned(),
                        },
                        execution_timeout_seconds: Some(120),
                        model: Some("fixture-model".into()),
                        effort: Some("medium".into()),
                    },
                    imported_continuity: ContinuityInput::None,
                    now_ms: 0,
                },
            )
            .await?;
        let run_id = store
            .lock()
            .await
            .enqueue_due_run::<SessionRef, EndpointRef, message_board::ProjectId>(
                &schedule.schedule_id,
                60000,
            )
            .await?
            .ok_or("run missing")?;
        store
            .lock()
            .await
            .admit_waiting_run::<SessionRef, EndpointRef, message_board::ProjectId>(
                &schedule.schedule_id,
                60000,
            )
            .await?;
        let worker = ScheduledRunWorker {
            store: Arc::clone(&store),
            interactions: Arc::clone(&interactions),
            execution: Arc::clone(&fake),
            backend: None,
            configuration: collaboration_runtime::AutomationConfigurationHandle::default(),
            machine_identity: collaboration_contracts::MachineIdentity::new(
                target.endpoint.service_id.clone(),
                Some("Schedule Fixture"),
            )?,
        };
        for stage in 0..3 {
            if let Err(error) = Box::pin(worker.step(run_id.clone())).await {
                let current = store
                    .lock()
                    .await
                    .read_run::<SessionRef, EndpointRef, message_board::ProjectId, CodexGeneration, collaboration_runtime::stored_run_receipt::StoredRunReceipt>(&run_id)
                    .await?;
                return Err(format!(
                    "fake {route_name} stage {stage}: {error}; phase={:?}, route={:?}, timing={:?}",
                    current.phase, current.evidence.route, current.evidence.timing
                )
                .into());
            }
            apply_push_handoff(&worker).await?;
        }
        if matches!(kind, FakeRouteKind::Provider) {
            Box::pin(worker.step(run_id.clone()))
                .await
                .map_err(|error| format!("fake provider settle: {error}"))?;
        }
        let identity = crate::ApplicationFixture::new(service, service)?
            .with_automation_store(Arc::clone(&store))
            .with_interaction_store(interactions)
            .with_scheduled_run_execution(fake);
        let application = (identity).build();
        let shown = application
            .automation()
            .run_show(RunShowRequest { run_id })
            .await
            .map_err(|failure| format!("run show failed: {failure:?}"))?;
        match (kind, shown.state) {
            (
                FakeRouteKind::Provider,
                collaboration_protocol::RunState::Finished {
                    execution: RunExecution::ProviderAcp { .. },
                    ..
                },
            )
            | (
                FakeRouteKind::Peer,
                collaboration_protocol::RunState::Finished {
                    execution: RunExecution::ClaudeCodePeer { .. },
                    ..
                },
            ) => {}
            _ => return Err("run/show lost the selected route execution".into()),
        }
        if shown.summary.is_some() || shown.execution_evidence.route.is_none() {
            return Err("run/show invented a summary or lost route evidence".into());
        }
        drop(application);
        drop(worker);
        drop(store);
        for entry in std::fs::read_dir(&root)? {
            std::fs::remove_file(entry?.path())?;
        }
        std::fs::remove_dir(root)?;
    }
    Ok(())
}

#[tokio::test]
async fn provider_existing_preparation_is_inspectable_and_fresh_activation_is_rejected()
-> TestResult<()> {
    let root = std::env::temp_dir().join(format!(
        "provider-preparation-{}",
        OperationId::generate().as_str()
    ));
    std::fs::create_dir(&root)?;
    let store = Arc::new(Mutex::new(
        AutomationStore::open(&root.join("automation.sqlite")).await?,
    ));
    let service = "00000000-0000-4000-8000-000000000001";
    let target: SessionRef = serde_json::from_value(json!({
        "endpoint":{"serviceId":service,"endpointId":"claude-local"},
        "sessionId":"prepared-provider"
    }))?;
    let interactions = Arc::new(
        interaction_storage::InteractionStore::open(root.join("interaction.sqlite")).await?,
    );
    let generation: CodexGeneration =
        serde_json::from_value(json!({"serviceEpoch":service,"generation":1}))?;
    let fake: Arc<dyn ScheduledRunExecution> = Arc::new(FakeScheduledExecution {
        kind: FakeRouteKind::Provider,
        target: target.clone(),
        interactions,
        observed_push_ids: Arc::new(std::sync::Mutex::new(Vec::new())),
        generation,
        attempt_id: AttemptId::generate(),
        submissions: AtomicUsize::new(0),
        submission_plan: FakeSubmissionPlan::Accept,
        settlement_plan: FakeSettlementPlan::Completed,
    });
    let instruction = store
        .lock()
        .await
        .create_instruction(
            &OperationId::generate(),
            &InstructionText::try_from("Check job".to_owned())?,
            0,
        )
        .await?;
    let mut schedules = Vec::new();
    for _ in 0..2 {
        let schedule = store
            .lock()
            .await
            .create_schedule(
                &ScheduleCreate::<SessionRef, EndpointRef, message_board::ProjectId> {
                    operation_id: OperationId::generate(),
                    definition: ScheduleDefinition {
                        instruction_id: instruction.instruction_id.clone(),
                        timing: TimingRule::Interval { seconds: 60 },
                        enabled: false,
                        destination: ExecutionDestination::Unprepared { project_id: None },
                        execution_timeout_seconds: Some(120),
                        model: Some("fixture-model".into()),
                        effort: Some("medium".into()),
                    },
                    imported_continuity: ContinuityInput::None,
                    now_ms: 0,
                },
            )
            .await?;
        schedules.push(schedule.schedule_id);
    }
    let identity = crate::ApplicationFixture::new(service, service)?
        .with_automation_store(Arc::clone(&store))
        .with_scheduled_run_execution(fake);
    let application = (identity).build();
    let fresh = application
        .automation()
        .schedule_create(serde_json::from_value(json!({
            "operationId": OperationId::generate(),
            "definition": {
                "instructionId": instruction.instruction_id.clone(),
                "timing": {"kind": "interval", "seconds": 60},
                "enabled": true,
                "destination": {"kind": "freshEachRun", "endpoint": target.endpoint.clone(), "cwd": root},
                "executionTimeoutSeconds": 120,
                "model": "fixture-model",
                "effort": "medium"
            }
        }))?)
        .await;
    if !matches!(fresh.as_ref().map_err(crate::collaboration_application::ScheduleOperationFailure::failure), Err(failure)
        if failure.field.as_deref() == Some("destination")
            && failure.constraint.as_deref().is_some_and(|fix| fix.contains("conversation create")))
    {
        return Err("fresh provider schedule activation lacked the create-first fix".into());
    }
    let operation_id = OperationId::generate();
    let prepared = application
        .automation()
        .schedule_prepare(serde_json::from_value(json!({
            "operationId":operation_id,"scheduleId":schedules[0],
            "destination":{"kind":"existing","target":target.clone(),"cwd":root}
        }))?)
        .await?;
    if !matches!(prepared.definition.destination, collaboration_protocol::ExecutionDestination::OwnedThread { target: prepared_target, .. } if prepared_target == target)
    {
        return Err("provider preparation did not bind the prepared target".into());
    }
    let shown = application
        .automation()
        .operation_show(
            collaboration_protocol::OperationShowRequest { operation_id },
            crate::collaboration_application::API_RESULT_BUDGET,
        )
        .await
        .map_err(|failure| format!("operation show failed: {failure:?}"))?;
    if !matches!(
        shown.state,
        collaboration_protocol::OperationState::Succeeded { .. }
    ) {
        return Err("provider preparation did not complete in operation/show".into());
    }
    let fork_operation = OperationId::generate();
    let fork = application
        .automation()
        .schedule_prepare(serde_json::from_value(json!({
            "operationId":fork_operation,"scheduleId":schedules[1],
            "destination":{"kind":"fork","source":target,"throughTurnId":"turn-one","cwd":root}
        }))?)
        .await;
    if !matches!(fork.as_ref().map_err(crate::collaboration_application::ScheduleOperationFailure::failure), Err(failure)
        if matches!(failure.kind, collaboration_protocol::ScheduleFailureKind::UnsupportedCapability)
            && matches!(failure.effects, collaboration_protocol::ScheduleEffects::Route { ref evidence } if matches!(evidence.as_ref(), DeliveryRouteEvidence::ProviderAcp { .. })))
    {
        return Err("provider fork did not report route-specific unsupported capability".into());
    }
    let fork_shown = application
        .automation()
        .operation_show(
            collaboration_protocol::OperationShowRequest {
                operation_id: fork_operation,
            },
            crate::collaboration_application::API_RESULT_BUDGET,
        )
        .await
        .map_err(|failure| format!("operation show failed: {failure:?}"))?;
    if !matches!(fork_shown.state, collaboration_protocol::OperationState::Failed { ref error }
        if matches!(&error.effects, collaboration_protocol::OperationEffects::Route {
            evidence,
        } if matches!(**evidence, DeliveryRouteEvidence::ProviderAcp { .. })))
    {
        return Err("operation/show lost the provider preparation evidence".into());
    }
    drop(application);
    drop(store);
    for entry in std::fs::read_dir(&root)? {
        std::fs::remove_file(entry?.path())?;
    }
    std::fs::remove_dir(root)?;
    Ok(())
}

#[path = "scheduled_push_retention_tests.rs"]
mod retention_tests;
