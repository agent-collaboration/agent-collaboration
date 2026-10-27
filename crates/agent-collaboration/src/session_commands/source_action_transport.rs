//! One-shot native mutation exchange, separate from cancellable inventory reads.
//! A lost or misattributed receipt cannot establish whether the source acted.
use super::{
    router_connection_registry::RouterConnectionProfile,
    source_ssh_protocol::{
        SourceManagedAction, SourceNativeBinding, SourceNativeClaudeAction,
        SourceNativeClaudeIdentity, SourceNativeClaudeResult, SourceNativeObservation,
        SourceNativeUnknownReason, SourceOperation, SourceProviderActionResult,
        SourceProviderOperationRead, SourceReadError, SourceRejection, SourceReply,
    },
};
use collaboration_client::protocol::{
    ConversationCreateOutcome, ConversationOperationSettlement, ConversationOperationWaitOutput,
    OperationId, ProviderBindingIdentity, UuidIdentity,
};
use collaboration_client::{ConversationOperationResult, ConversationSettlementDetail};

pub(crate) async fn read_managed_operation(
    profile: &RouterConnectionProfile,
    service_epoch: UuidIdentity,
    operation_id: OperationId,
) -> Result<SourceProviderOperationRead, SourceReadError> {
    let reply = super::source_ssh_transport::read_source(
        profile,
        SourceOperation::ProviderOperationShow {
            service_epoch,
            operation_id: operation_id.clone(),
        },
    )
    .await?;
    let SourceReply::ProviderOperation {
        operation_id: actual_id,
        result,
    } = reply
    else {
        return Err(SourceReadError::InvalidReply);
    };
    if actual_id != operation_id {
        return Err(SourceReadError::InvalidReply);
    }
    match result.as_ref() {
        SourceProviderOperationRead::Found { operation }
            if operation.operation_id == operation_id
                && operation
                    .binding
                    .external_provider()
                    .is_some_and(|binding| binding.endpoint.service_id == profile.service_id) => {}
        SourceProviderOperationRead::Failed { failure }
            if failure
                .operation_id
                .as_ref()
                .is_none_or(|actual| actual == &operation_id)
                && failure
                    .target
                    .as_ref()
                    .is_none_or(|target| target.endpoint.service_id == profile.service_id) => {}
        _ => return Err(SourceReadError::InvalidReply),
    }
    Ok(*result)
}

pub(crate) async fn submit_managed_source(
    profile: &RouterConnectionProfile,
    operation_id: OperationId,
    binding: ProviderBindingIdentity,
    action: SourceManagedAction,
) -> Result<SourceProviderActionResult, SourceReadError> {
    let target = match &action {
        SourceManagedAction::Create { .. } => None,
        SourceManagedAction::Load { target } | SourceManagedAction::Resume { target } => {
            Some(target.clone())
        }
    };
    if binding.endpoint.service_id != profile.service_id
        || target
            .as_ref()
            .is_some_and(|target| target.endpoint != binding.endpoint)
    {
        return Err(SourceReadError::Rejected(SourceRejection::WrongService));
    }
    let reply = super::source_ssh_transport::exchange_source(
        profile,
        SourceOperation::ProviderAction {
            operation_id: operation_id.clone(),
            binding: binding.clone(),
            action: action.clone(),
        },
    )
    .await;
    match reply {
        Ok(SourceReply::ProviderAction { result })
            if managed_reply_matches(&result, &operation_id, &binding, &action) =>
        {
            Ok(*result)
        }
        Err(error @ SourceReadError::Rejected(_)) => Err(error),
        _ => Ok(SourceProviderActionResult::Unknown {
            operation_id,
            target,
        }),
    }
}

fn managed_reply_matches(
    result: &SourceProviderActionResult,
    operation_id: &OperationId,
    binding: &ProviderBindingIdentity,
    action: &SourceManagedAction,
) -> bool {
    let target = match action {
        SourceManagedAction::Create { .. } => None,
        SourceManagedAction::Load { target } | SourceManagedAction::Resume { target } => {
            Some(target)
        }
    };
    match result {
        SourceProviderActionResult::ManagedCreate { result }
            if matches!(action, SourceManagedAction::Create { .. }) =>
        {
            match result.as_ref() {
                ConversationCreateOutcome::Created {
                    operation_id: actual,
                    target,
                    ..
                }
                | ConversationCreateOutcome::CreatedWithoutSettings {
                    operation_id: actual,
                    target,
                    ..
                } => actual == operation_id && target.endpoint == binding.endpoint,
                ConversationCreateOutcome::Pending {
                    operation_id: actual,
                } => actual == operation_id,
            }
        }
        SourceProviderActionResult::ManagedLoad { result }
            if matches!(action, SourceManagedAction::Load { .. }) =>
        {
            match result.as_ref() {
                ConversationOperationResult::Completed {
                    target: actual,
                    operation_id: Some(actual_id),
                    settlement,
                } => {
                    actual_id == operation_id
                        && Some(actual) == target
                        && settlement.target == *actual
                        && matches!(
                            settlement.detail,
                            ConversationSettlementDetail::ProviderLoad { .. }
                        )
                }
                ConversationOperationResult::Pending {
                    operation_id: actual_id,
                    target: actual,
                } => actual_id == operation_id && Some(actual) == target,
                _ => false,
            }
        }
        SourceProviderActionResult::ManagedResume { result }
            if matches!(action, SourceManagedAction::Resume { .. }) =>
        {
            result.operation.operation_id == *operation_id
                && result.operation.target.as_ref() == target
                && result.operation.binding.external_provider() == Some(binding)
                && match &result.output {
                    ConversationOperationWaitOutput::Available {
                        settlement: ConversationOperationSettlement::Resumed { target: actual, .. },
                    } => Some(actual) == target,
                    ConversationOperationWaitOutput::Pending
                    | ConversationOperationWaitOutput::OutputUnavailable { .. } => true,
                    ConversationOperationWaitOutput::Available { .. } => false,
                }
        }
        SourceProviderActionResult::Failed {
            operation_id: actual,
            target: actual_target,
            failure,
        } => {
            actual == operation_id
                && actual_target.as_ref() == target
                && failure
                    .operation_id
                    .as_ref()
                    .is_none_or(|actual| actual == operation_id)
                && failure.target.as_ref().is_none_or(|actual| {
                    actual.endpoint == binding.endpoint
                        && target.is_none_or(|target| target == actual)
                })
        }
        SourceProviderActionResult::Unknown {
            operation_id: actual,
            target: actual_target,
        } => actual == operation_id && actual_target.as_ref() == target,
        _ => false,
    }
}

pub(crate) async fn submit_native_source(
    profile: &RouterConnectionProfile,
    service_epoch: UuidIdentity,
    action: SourceNativeClaudeAction,
) -> Result<SourceNativeClaudeResult, SourceReadError> {
    let identity = action.captured_identity();
    let target = match &identity {
        SourceNativeClaudeIdentity::New { .. } => None,
        SourceNativeClaudeIdentity::Resume { target, .. }
        | SourceNativeClaudeIdentity::Fork { target, .. } => Some(target),
    };
    if target.is_some_and(|target| target.endpoint.service_id != profile.service_id) {
        return Err(SourceReadError::Rejected(SourceRejection::WrongService));
    }
    let source = SourceNativeBinding {
        service_id: profile.service_id.clone(),
        service_epoch: service_epoch.clone(),
    };
    let reply = super::source_ssh_transport::exchange_source(
        profile,
        SourceOperation::NativeClaude {
            service_epoch,
            action,
        },
    )
    .await;
    match reply {
        Ok(SourceReply::NativeClaude { result })
            if result.source == source && result.action == identity =>
        {
            Ok(*result)
        }
        Err(error @ SourceReadError::Rejected(_)) => Err(error),
        // Malformed, lost, oversized, nonzero or wrong-target replies all lose
        // the observation. None authorize a second submission or Router lookup.
        _ => Ok(SourceNativeClaudeResult {
            source,
            action: identity,
            observation: SourceNativeObservation::Unknown {
                reason: SourceNativeUnknownReason::TransportUnavailable,
                status: None,
                stdout: Vec::new(),
                stderr: Vec::new(),
            },
        }),
    }
}

#[cfg(test)]
#[path = "source_action_transport_tests.rs"]
mod tests;
