//! Target-owned native effects bind central truth; retries replay the retained exact Bind.
use super::AutomationOperations;
use crate::schedule_preparation::{
    destination_cwd, destination_endpoint, error, reject_preselection,
};
use crate::{
    CentralCatalogFailure, ScheduleCommandFailure,
    collaboration_application::ScheduleOperationFailure,
};
use agent_automation::RouteEffectEvidence;
use automation_storage::{
    ExternalAdmission, ExternalAdmissionResult, ScheduleInspection, StorageError,
    StoredCentralBindRequest,
};
use collaboration_contracts::{
    PreparationEvidenceSink, ScheduleDestination, SchedulePreparationOutcome,
    SchedulePreparationRequest,
};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationKind, BoundScheduleThread, CodexGeneration,
    DestinationPreparation, EndpointRef, ExecutionDestination, ImportedContinuity,
    InstructionShowParams, LocalMutationState, ScheduleBindPreparedRequest, ScheduleEffects,
    ScheduleFailure, ScheduleFailureDetails, ScheduleFailureKind, ScheduleFailureStage,
    ScheduleNextAction, SchedulePrepareRequest, ScheduleShowRequest, ScheduleSnapshot, SessionRef,
};
use collaboration_runtime::schedule_preparation_evidence_sink::{
    StoredPreparationEvidenceSink, same_preparation_identity, validate_prepared_target,
};

pub(super) type Evidence = RouteEffectEvidence<SessionRef, CodexGeneration>;
pub(super) type PreparationResult = Result<ScheduleSnapshot, ScheduleCommandFailure>;

impl AutomationOperations {
    pub(super) async fn prepare_central_schedule(
        &self,
        params: SchedulePrepareRequest,
    ) -> PreparationResult {
        let canonical = serde_json::to_vec(&(&params.schedule_id, &params.destination))
            .map_err(|_| invalid(&params, "Preparation request cannot be encoded."))?;
        let store = self
            .store()
            .ok_or_else(|| invalid(&params, "Automation storage is unavailable."))?;
        // Replay selection precedes catalog reads and all native calls, including capability checks.
        let retained = store
            .lock()
            .await
            .read_schedule_preparation::<Evidence, ScheduleInspection<SessionRef, EndpointRef>>(
                &params.operation_id,
                &params.schedule_id,
                &canonical,
            )
            .await
            .map_err(|failure| storage_failure(&params, failure))?;
        if let Some(record) = retained {
            return self.replay_central_preparation(&params, record).await;
        }
        let schedule = match self
            .catalog
            .request(
                AutomationCall::ScheduleShow(ScheduleShowRequest {
                    schedule_id: params.schedule_id.clone(),
                }),
                None,
            )
            .await.map_err(|failure|preparation_read_failure(&params,failure))?
        {
            AutomationAnswer::Suspended(value) if value.kind == AutomationKind::Schedule && value.automation_id.as_str() == params.schedule_id.as_str() => return reject_preselection(
                &params,ScheduleFailureKind::InvalidField,
                "Schedule is suspended; repair its timing before preparing a native target. No native effect was dispatched.",None,
            ).map_err(Into::into),
            AutomationAnswer::Schedule(schedule) if schedule.schedule_id == params.schedule_id => {
                schedule
            }
            _ => return Err(CentralCatalogFailure::InvalidReply.into()),
        };
        let destination = route_destination(&params.destination);
        let effects = match &self.scheduled_run_execution {
            Some(execution) => execution
                .initial_evidence(&destination)
                .await
                .ok()
                .filter(Evidence::is_valid),
            None => None,
        };
        if !destination_cwd(&params.destination).starts_with('/')
            || destination_cwd(&params.destination).contains('\0')
        {
            return reject_preselection(
                &params,
                ScheduleFailureKind::InvalidField,
                "Use an absolute Unix workspace path on the target machine.",
                effects.as_ref(),
            )
            .map_err(Into::into);
        }
        let target_service = &destination_endpoint(&params.destination).service_id;
        let schedule_target = match &schedule.definition.destination {
            ExecutionDestination::OwnedThread { target, .. } => Some(&target.endpoint.service_id),
            ExecutionDestination::FreshEachRun { endpoint, .. } => Some(&endpoint.service_id),
            ExecutionDestination::Unprepared | ExecutionDestination::FreshEachRunUnprepared => None,
        };
        if target_service != self.identity.service_id()
            || schedule_target.is_some_and(|target| target != self.identity.service_id())
        {
            let target = String::from(schedule_target.unwrap_or(target_service).clone());
            return reject_preselection(
                &params, ScheduleFailureKind::InvalidField,
                &format!("Run schedule_prepare on target machine {target}; remote preparation is unavailable and no native effect was dispatched."),
                effects.as_ref(),
            ).map_err(Into::into);
        }
        if matches!(
            schedule.definition.destination,
            ExecutionDestination::FreshEachRun { .. }
                | ExecutionDestination::FreshEachRunUnprepared
        ) {
            return reject_preselection(&params,ScheduleFailureKind::InvalidField,"Execution mode is fixed at creation. Schedule prepare binds a reuse-thread destination; no native effect was dispatched.",effects.as_ref()).map_err(Into::into);
        }
        if !matches!(
            schedule.definition.destination,
            ExecutionDestination::Unprepared
        ) {
            return reject_preselection(&params,ScheduleFailureKind::InvalidField,"Only an unprepared reuse-thread Schedule can be prepared; no native effect was dispatched.",effects.as_ref()).map_err(Into::into);
        }
        for (field, value, required) in [
            ("effort", schedule.definition.effort.as_ref(), true),
            (
                "model",
                schedule.definition.model.as_ref(),
                !matches!(params.destination, DestinationPreparation::Existing { .. }),
            ),
        ] {
            if required
                && !value.is_some_and(|value| {
                    !value.trim().is_empty() && !value.chars().any(char::is_whitespace)
                })
            {
                return crate::schedule_preparation::reject_choice_field(
                    &params,
                    field,
                    effects.as_ref(),
                )
                .map_err(Into::into);
            }
        }
        let Some(execution) = &self.scheduled_run_execution else {
            return reject_preselection(
                &params,
                ScheduleFailureKind::UnsupportedCapability,
                "No native preparation route is available; no native effect was dispatched.",
                effects.as_ref(),
            )
            .map_err(Into::into);
        };
        let support = execution.support(&destination).await;
        if let Ok(collaboration_contracts::ScheduleSupport::Unsupported { missing }) = &support {
            let create_first = matches!(destination, ScheduleDestination::Fresh { .. })
                && missing.contains(&collaboration_contracts::ScheduleCapability::CreateSession)
                && !matches!(effects, Some(RouteEffectEvidence::CodexAppServer(_)));
            return reject_preselection(&params,
                if create_first {ScheduleFailureKind::InvalidField} else {ScheduleFailureKind::UnsupportedCapability},
                if create_first {"create the session first (conversation create), then schedule it as an existing target"}
                else {"This target does not support preparation; no native effect was dispatched."},effects.as_ref()).map_err(Into::into);
        }
        if support.is_err() || effects.is_none() {
            return reject_preselection(&params,ScheduleFailureKind::UnsupportedCapability,"Target preparation capabilities could not be established; no native effect was dispatched.",effects.as_ref()).map_err(Into::into);
        }
        let effects =
            effects.ok_or_else(|| invalid(&params, "Missing selected route evidence."))?;
        let instruction = match self
            .catalog
            .request(
                AutomationCall::InstructionShow(InstructionShowParams {
                    instruction_id: schedule.definition.instruction_id.clone(),
                }),
                None,
            )
            .await
            .map_err(|failure| preparation_read_failure(&params, failure))?
        {
            AutomationAnswer::Instruction(instruction)
                if instruction.instruction_id == schedule.definition.instruction_id =>
            {
                instruction
            }
            _ => return Err(CentralCatalogFailure::InvalidReply.into()),
        };
        store
            .lock()
            .await
            .project_central_preparation(&schedule, &instruction)
            .await
            .map_err(|failure| storage_failure(&params, failure))?;
        let admitted = store
            .lock()
            .await
            .admit_schedule_preparation::<_, ScheduleInspection<SessionRef, EndpointRef>>(
                &ExternalAdmission {
                    operation_id: params.operation_id.clone(),
                    schedule_id: params.schedule_id.clone(),
                    canonical_request: canonical,
                    evidence: effects.clone(),
                    now_ms: chrono::Utc::now().timestamp_millis(),
                },
            )
            .await
            .map_err(|failure| storage_failure(&params, failure))?;
        if let ExternalAdmissionResult::Existing(record) = admitted {
            return self.replay_central_preparation(&params, record).await;
        }
        let mut text = instruction.text.as_str().to_owned();
        if let ImportedContinuity::ImportedSummary { text: summary, .. } =
            &schedule.imported_continuity
        {
            text.push_str("\n\nPrior scheduled-work summary (context, not proof):\n");
            text.push_str(summary);
        }
        let sink = StoredPreparationEvidenceSink {
            store: store.clone(),
            configuration: self.configuration.clone(),
            operation_id: params.operation_id.clone(),
            schedule_id: params.schedule_id.clone(),
            expected_change_id: schedule.change_id.clone(),
            working_directory: destination_cwd(&params.destination).to_owned(),
        };
        let outcome = execution
            .prepare_destination(
                SchedulePreparationRequest {
                    operation_id: params.operation_id.clone(),
                    schedule_id: params.schedule_id.clone(),
                    expected_change_id: schedule.change_id.clone(),
                    destination: params.destination.clone(),
                    instruction_text: text,
                    model: schedule.definition.model,
                    effort: schedule.definition.effort.unwrap_or_default(),
                },
                &sink,
            )
            .await;
        match outcome {
            Ok(SchedulePreparationOutcome::Prepared(prepared)) => {
                let prior = store.lock().await.read_operation(&params.operation_id).await;
                let identity_matches = match prior {
                    Ok(automation_storage::StoredOperationRecord {
                        state: automation_storage::StoredOperationState::InProgress { effects: prior }, ..
                    }) => collaboration_runtime::schedule_preparation_evidence_sink::decode_stored_evidence(prior)
                        .is_ok_and(|prior| same_preparation_identity(&prior, &prepared.evidence)),
                    _ => false,
                };
                if validate_prepared_target(&prepared).is_err() || !identity_matches {
                    return crate::schedule_preparation::finish_failure(
                        store, &params, ScheduleFailureKind::OutcomeUnknown,
                        "Prepared target identity could not be established from the retained intent; inspect this operation.",
                        if prepared.evidence.is_valid() { prepared.evidence } else { effects }, true,
                    ).await.map_err(Into::into);
                }
                let bind = StoredCentralBindRequest::encode(&ScheduleBindPreparedRequest {
                    automation_id: params.schedule_id.as_str().to_owned().try_into()
                        .map_err(|_| invalid(&params, "Invalid Schedule identity."))?,
                    expected_revision: schedule.change_id,
                    operation_id: params.operation_id.clone(),
                    bound_thread: BoundScheduleThread {
                        target: prepared.target,
                        cwd: destination_cwd(&params.destination).to_owned(),
                    },
                });
                let retained = match &bind {
                    Ok(bind) => store.lock().await.record_central_bind_request(
                        bind, &prepared.evidence, chrono::Utc::now().timestamp_millis(),
                    ).await,
                    Err(_) => Err(StorageError::InvalidRecord),
                };
                if let Err(failure) = retained {
                    let ownership_conflict = matches!(failure, StorageError::ThreadOwnershipConflict { .. });
                    return crate::schedule_preparation::finish_failure(
                        store, &params,
                        if ownership_conflict { ScheduleFailureKind::OwnershipConflict } else { ScheduleFailureKind::OutcomeUnknown },
                        "Native preparation completed but its Bind request was not retained; inspect the operation and target before retrying.",
                        prepared.evidence, !ownership_conflict,
                    ).await.map_err(Into::into);
                }
                let bind = bind.map_err(|_| invalid(&params, "Retained Bind encoding is unavailable."))?;
                self.dispatch_retained_bind(&params, &bind, prepared.evidence).await
            }
            Ok(SchedulePreparationOutcome::Failed(failure)) => {
                let public = error(&params, failure.kind, &failure.explanation, failure.evidence.clone());
                sink.record_failure(&public, failure.evidence.clone(), failure.uncertain).await
                    .map_err(|_| ScheduleOperationFailure::Preparation(error(
                        &params, ScheduleFailureKind::OutcomeUnknown,
                        "Preparation failure could not be retained; inspect the original operation.", failure.evidence,
                    )))?;
                Err(ScheduleOperationFailure::Preparation(public).into())
            }
            Err(_) => crate::schedule_preparation::finish_failure(
                store, &params, ScheduleFailureKind::OutcomeUnknown,
                "Preparation failed after admission; inspect the original operation before retrying.", effects, true,
            ).await.map_err(Into::into),
        }
    }
}

fn route_destination(destination: &DestinationPreparation) -> ScheduleDestination {
    match destination {
        DestinationPreparation::Existing { target, .. } => ScheduleDestination::Existing {
            target: target.clone(),
        },
        DestinationPreparation::Fresh { endpoint, .. } => ScheduleDestination::Fresh {
            endpoint: endpoint.clone(),
        },
        DestinationPreparation::Fork {
            source,
            through_turn_id,
            ..
        } => ScheduleDestination::Fork {
            source: source.clone(),
            through_turn_id: through_turn_id.clone(),
        },
    }
}
pub(super) fn invalid(params: &SchedulePrepareRequest, message: &str) -> ScheduleCommandFailure {
    ScheduleOperationFailure::Preparation(ScheduleFailure {
        kind: ScheduleFailureKind::InvalidRecord,
        stage: ScheduleFailureStage::Storage,
        message: message.into(),
        operation_id: Some(params.operation_id.clone()),
        schedule_id: Some(params.schedule_id.clone()),
        current_change_id: None,
        field: None,
        constraint: None,
        details: ScheduleFailureDetails::None,
        effects: ScheduleEffects::Local {
            mutation: LocalMutationState::None,
        },
        next_action: ScheduleNextAction::InspectOperation,
    })
    .into()
}
pub(super) fn storage_failure(
    params: &SchedulePrepareRequest,
    failure: StorageError,
) -> ScheduleCommandFailure {
    ScheduleOperationFailure::Preparation(crate::schedule_command_failure::schedule_failure(
        crate::schedule_command_failure::ScheduleFailureContext::new(
            Some(params.operation_id.clone()),
            Some(params.schedule_id.clone()),
        ),
        failure,
        LocalMutationState::None,
        None,
    ))
    .into()
}

fn preparation_read_failure(
    params: &SchedulePrepareRequest,
    failure: CentralCatalogFailure,
) -> CentralCatalogFailure {
    match failure {
        CentralCatalogFailure::CentralUnreachable { .. }
        | CentralCatalogFailure::OutcomeUnknown { .. } => {
            CentralCatalogFailure::CentralUnreachable {
                operation_id: Some(params.operation_id.clone()),
            }
        }
        CentralCatalogFailure::Refused { error, .. } => CentralCatalogFailure::Refused {
            operation_id: Some(params.operation_id.clone()),
            error,
        },
        CentralCatalogFailure::InvalidReply => CentralCatalogFailure::InvalidReply,
    }
}
