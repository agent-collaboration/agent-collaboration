//! Durable provider resume and close operations.

use super::provider_operation_admission::DispatchMark;
use super::{
    ConversationOperationFailureKind, ConversationOperationFailureStage,
    ConversationOperationSubmission, ExternalProviderSupervisor, PreparedOperation,
    ProviderConversationFuture, ProviderOperationEffect, ProviderOperationKind,
    admitted_submission, existing_submission, failure, provider_session_record, retain_operation,
};
use collaboration_protocol::{ConversationCloseRequest, ConversationResumeRequest};

pub(super) fn resume(
    supervisor: &ExternalProviderSupervisor,
    request: ConversationResumeRequest,
) -> ProviderConversationFuture<'_, ConversationOperationSubmission> {
    let operation_id = request.operation_id.clone();
    let failure_target = Some(request.target.clone());
    let deadline = supervisor.admission_deadline();
    let backend = supervisor.clone();
    retain_operation(operation_id, failure_target, async move {
        let target = request.target.clone();
        let (binding, runtime) = backend.runtime_binding(
            &target.endpoint,
            &request.operation_id,
            Some(target.clone()),
        )?;
        let provider_session_id = String::from(target.session_id.clone());
        if !backend
            .within_admission(
                deadline,
                &request.operation_id,
                Some(&target),
                runtime.capability_report(&provider_session_id),
            )
            .await?
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
                deadline,
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
                let record = Box::new(provider_session_record(
                    target,
                    request.working_directory,
                    request.requested_policy,
                    request.requested_by,
                    request.approver,
                    collaboration_runtime::ProviderSessionResumeInputs::default(),
                    backend.inner.now_ms(),
                ));
                let completion_backend = backend.clone();
                backend.spawn_operation_for_handoff(operation_id.clone(), live, async move {
                    super::provider_owner_operations::restore(
                        &completion_backend,
                        operation_id,
                        record,
                        provider_session_machines::session_lifecycle_machine::RestoreVia::Resume,
                    )
                    .await
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
    let deadline = supervisor.admission_deadline();
    let backend = supervisor.clone();
    retain_operation(operation_id, failure_target, async move {
        let target = request.target.clone();
        let (binding, runtime) = backend.runtime_binding(
            &target.endpoint,
            &request.operation_id,
            Some(target.clone()),
        )?;
        let provider_session_id = String::from(target.session_id.clone());
        // An agent with no session/close is closed by the Router itself: the
        // same interrupt, then a release here (owner: "close should just
        // interrupt the agent and close", every agent).
        let agent_closes = backend
            .within_admission(
                deadline,
                &request.operation_id,
                Some(&target),
                runtime.capability_report(&provider_session_id),
            )
            .await?
            .supports_close;
        // Close interrupts a running turn (D1 R14): the live turn's row
        // doesn't refuse it; an earlier outcome that is unknown and no longer
        // live still does.
        let prepared = backend
            .admit_operation(
                deadline,
                request.operation_id.clone(),
                ProviderOperationKind::ConversationClose,
                binding,
                Some(&target),
                DispatchMark::BeforeHandoff,
            )
            .await?;
        match prepared {
            PreparedOperation::Existing(operation) => Ok(existing_submission(operation)),
            PreparedOperation::Admitted { snapshot, live } => {
                let operation_id = request.operation_id.clone();
                let completion_backend = backend.clone();
                backend.spawn_operation_for_handoff(operation_id.clone(), live, async move {
                    super::provider_owner_operations::close(
                        &completion_backend,
                        operation_id,
                        target,
                        agent_closes,
                    )
                    .await
                });
                Ok(admitted_submission(snapshot))
            }
        }
    })
}
