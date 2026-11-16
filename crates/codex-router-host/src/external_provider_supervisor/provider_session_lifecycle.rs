//! Durable provider resume and close operations.

use super::provider_operation_admission::DispatchMark;
use super::{
    Arc, ConversationOperationFailureKind, ConversationOperationFailureStage,
    ConversationOperationSettlement, ConversationOperationSubmission, ExternalProviderSupervisor,
    PathBuf, PreparedOperation, ProviderConversationFuture, ProviderOperationCompletion,
    ProviderOperationEffect, ProviderOperationKind, admitted_submission, effective_settings,
    existing_submission, failure, provider_session_record, retain_operation, runtime_failure,
};
use collaboration_protocol::{
    ConversationCloseRequest, ConversationResumeRequest, ProviderHistoryAvailability,
};

pub(super) fn resume(
    supervisor: &ExternalProviderSupervisor,
    request: ConversationResumeRequest,
) -> ProviderConversationFuture<'_, ConversationOperationSubmission> {
    let operation_id = request.operation_id.clone();
    let failure_target = Some(request.target.clone());
    let backend = supervisor.clone();
    retain_operation(operation_id, failure_target, async move {
        let target = request.target.clone();
        let (binding, runtime) = backend.runtime_binding(
            &target.endpoint,
            &request.operation_id,
            Some(target.clone()),
        )?;
        let provider_session_id = String::from(target.session_id.clone());
        if !runtime
            .capability_report(&provider_session_id)
            .await
            .supports_resume
        {
            return Err(failure(
                ConversationOperationFailureKind::UnsupportedCapability,
                ConversationOperationFailureStage::Binding,
                ProviderOperationEffect::None,
                "provider runtime does not advertise conversation resume",
                request.operation_id,
                Some(target),
            ));
        }
        let prepared = backend
            .prepare_operation(
                request.operation_id.clone(),
                ProviderOperationKind::ConversationResume,
                binding,
                Some(&target),
            )
            .await?;
        match prepared {
            PreparedOperation::Existing(operation) => Ok(existing_submission(operation)),
            PreparedOperation::Admitted { snapshot, live } => {
                let operation_id = request.operation_id.clone();
                let working_directory = request.working_directory;
                let requested_policy = request.requested_policy;
                let created_by = request.requested_by;
                let approver = request.approver;
                let completion_target = target.clone();
                let completion_inner = Arc::clone(&backend.inner);
                backend.spawn_operation(operation_id.clone(), live, async move {
                    match runtime
                        .resume_session(
                            provider_session_id.clone(),
                            PathBuf::from(String::from(working_directory.clone())),
                        )
                        .await
                    {
                        Ok(()) => {
                            let mut observed = effective_settings(requested_policy.clone());
                            completion_inner
                                .history_unavailable
                                .lock()
                                .await
                                .insert(completion_target.clone());
                            if let Some(catalog) =
                                runtime.settings_catalog(&provider_session_id).await
                            {
                                let effective = catalog.effective_settings();
                                observed.mode = effective.mode;
                                observed.model = effective.model;
                                observed.effort = effective.effort;
                                observed.fast = effective
                                    .fast
                                    .map(crate::provider_operation_settlement::provider_fast_state);
                                completion_inner
                                    .record_settings_catalog(completion_target.clone(), catalog)
                                    .await;
                            }
                            ProviderOperationCompletion::Success {
                                settlement: ConversationOperationSettlement::Resumed {
                                    target: completion_target.clone(),
                                    effective_settings: observed,
                                    history: ProviderHistoryAvailability::HistoryUnavailable,
                                },
                                target: Some(completion_target.clone()),
                                session_record: Some(Box::new(provider_session_record(
                                    completion_target,
                                    working_directory,
                                    requested_policy,
                                    created_by,
                                    approver,
                                ))),
                            }
                        }
                        Err(error) => ProviderOperationCompletion::Failure(runtime_failure(
                            operation_id,
                            Some(completion_target),
                            error,
                        )),
                    }
                });
                Ok(admitted_submission(snapshot))
            }
        }
    })
}

pub(super) fn close(
    supervisor: &ExternalProviderSupervisor,
    request: ConversationCloseRequest,
) -> ProviderConversationFuture<'_, ConversationOperationSubmission> {
    let operation_id = request.operation_id.clone();
    let failure_target = Some(request.target.clone());
    let backend = supervisor.clone();
    retain_operation(operation_id, failure_target, async move {
        let target = request.target.clone();
        let (binding, runtime) = backend.runtime_binding(
            &target.endpoint,
            &request.operation_id,
            Some(target.clone()),
        )?;
        let provider_session_id = String::from(target.session_id.clone());
        if !runtime
            .capability_report(&provider_session_id)
            .await
            .supports_close
        {
            return Err(failure(
                ConversationOperationFailureKind::UnsupportedCapability,
                ConversationOperationFailureStage::Binding,
                ProviderOperationEffect::None,
                "provider runtime does not advertise conversation close",
                request.operation_id,
                Some(target),
            ));
        }
        // Close interrupts a running turn (D1 R14): the live turn's row
        // doesn't refuse it; an earlier outcome that is unknown and no longer
        // live still does.
        let prepared = backend
            .admit_operation(
                request.operation_id.clone(),
                ProviderOperationKind::ConversationClose,
                binding,
                Some(&target),
                DispatchMark::Deferred,
            )
            .await?;
        match prepared {
            PreparedOperation::Existing(operation) => Ok(existing_submission(operation)),
            PreparedOperation::Admitted { live, .. } => {
                let operation_id = request.operation_id.clone();
                let snapshot = backend.begin_dispatch(&operation_id, &target).await?;
                // Every waiting message ends not sent before the interrupt,
                // so none starts as the running turn ends.
                if let Some(fifo) = backend.message_fifo() {
                    fifo.close_session(&target, "session closed");
                }
                let completion_target = target.clone();
                // The runtime interrupts a running turn, waits for idle, then
                // closes.
                backend.spawn_operation(operation_id.clone(), live, async move {
                    match runtime.close_session(provider_session_id).await {
                        Ok(()) => ProviderOperationCompletion::Success {
                            settlement: ConversationOperationSettlement::Closed {
                                target: completion_target.clone(),
                            },
                            target: Some(completion_target),
                            session_record: None,
                        },
                        Err(error) => ProviderOperationCompletion::Failure(runtime_failure(
                            operation_id,
                            Some(completion_target),
                            error,
                        )),
                    }
                });
                Ok(admitted_submission(snapshot))
            }
        }
    })
}
