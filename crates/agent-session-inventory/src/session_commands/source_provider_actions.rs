//! Source-local managed mutations. Reads never submit these actions, and lost
//! submission output retains the caller operation rather than replaying it.
use super::{SourceRejection, SourceReply, SourceRequest, connect_expected};
use crate::sessions::source_ssh_protocol::{
    SourceManagedAction, SourceOperation, SourceProviderActionResult,
};
use collaboration_client::protocol::{
    ChannelDescription, CodexGeneration, ConversationOperationWaitRequest,
    ConversationResumeRequest, EndpointAvailability, NativeSessionScope, NativeSessionSource,
    NativeSessionView, OperationId, PositiveSeconds, ProviderBindingIdentity,
    ProviderCapabilityName, ProviderCapabilityStatus, ProviderIdentity, ProviderRequestedPolicy,
    ProviderSessionInspectRequest, ProviderSessionListParams, ProviderSessionSummary,
    ProviderWorkingDirectory, SessionRef,
};
use collaboration_client::{
    ClientError, CollaborationAccess, CollaborationClient, ConversationClient,
    ConversationClientError, ConversationCreateActor, ConversationCreateInput,
    ConversationLoadInput,
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::Duration,
};

struct StoredLoadMetadata {
    working_directory: ProviderWorkingDirectory,
    requested_policy: ProviderRequestedPolicy,
    approver: ProviderIdentity,
}

pub async fn inspect_stored_provider(
    request: &SourceRequest,
    binding: &ProviderBindingIdentity,
    target: &SessionRef,
) -> Result<SourceReply, SourceRejection> {
    if binding.endpoint.service_id != request.expected_service
        || target.endpoint != binding.endpoint
    {
        return Err(SourceRejection::WrongService);
    }
    let client = connect_expected(request).await?;
    validate_binding(&client, binding).await?;
    let _metadata = stored_load_metadata(&client, target).await?;
    let inspection = client
        .inspect_provider_session(ProviderSessionInspectRequest {
            target: target.clone(),
        })
        .await
        .map_err(|_| SourceRejection::SessionUnavailable)?;
    if inspection.target != *target {
        return Err(SourceRejection::SessionUnavailable);
    }
    validate_binding(&client, binding).await?;
    Ok(SourceReply::ProviderInspection {
        binding: Box::new(binding.clone()),
        inspection: Box::new(inspection),
    })
}

pub async fn execute_provider_action(
    request: &SourceRequest,
) -> Result<SourceReply, SourceRejection> {
    let SourceOperation::ProviderAction {
        operation_id,
        binding,
        action,
    } = &request.operation
    else {
        return Err(SourceRejection::InvalidRequest);
    };
    let target = match action {
        SourceManagedAction::Create { .. } => None,
        SourceManagedAction::Load { target } | SourceManagedAction::Resume { target } => {
            Some(target)
        }
    };
    if binding.endpoint.service_id != request.expected_service
        || target.is_some_and(|target| target.endpoint != binding.endpoint)
    {
        return Err(SourceRejection::WrongService);
    }
    let operation = async {
        let client = connect_expected(request).await?;
        validate_binding(&client, binding).await?;
        let human_id = collaboration_client::resolve_owner_human_id()
            .await
            .map_err(|_| SourceRejection::SourceUnavailable)?;
        let requester = ProviderIdentity::Human { human_id };
        let result = match action {
            SourceManagedAction::Create { directory } => {
                let access =
                    CollaborationAccess::api(Path::new(request.service_directory.as_str()));
                let conversation = ConversationClient::connect(&access, &binding.endpoint)
                    .await
                    .map_err(|_| SourceRejection::UnavailableEndpoint)?;
                validate_binding(&client, binding).await?;
                if !binding.capabilities.as_slice().iter().any(|entry| {
                    entry.name == ProviderCapabilityName::Create
                        && entry.status == ProviderCapabilityStatus::Supported
                }) {
                    return Err(SourceRejection::UnsupportedProviderAction);
                }
                let actor = ConversationCreateActor::Typed(
                    requester
                        .to_board_identity()
                        .map_err(|_| SourceRejection::InvalidRequest)?,
                );
                let input = ConversationCreateInput {
                    operation_id: operation_id.clone(),
                    endpoint: binding.endpoint.clone(),
                    working_directory: PathBuf::from(directory.as_str()),
                    access: collaboration_client::protocol::RouterAccess::WorkspaceWrite,
                    created_by: actor.clone(),
                    approver: Some(actor),
                    generation: Some(binding.generation.clone()),
                    model: None,
                    mode: None,
                    effort: None,
                    fork: None,
                    root_message_id: None,
                };
                match conversation.create(input, Duration::from_secs(10)).await {
                    Ok(result) => SourceProviderActionResult::ManagedCreate {
                        result: Box::new(result),
                    },
                    Err(ConversationClientError::OperationFailure(failure)) => {
                        SourceProviderActionResult::Failed {
                            operation_id: operation_id.clone(),
                            target: None,
                            failure,
                        }
                    }
                    Err(_) => unknown_result(operation_id, None),
                }
            }
            SourceManagedAction::Load { target } => {
                let access =
                    CollaborationAccess::api(Path::new(request.service_directory.as_str()));
                let conversation = ConversationClient::connect(&access, &target.endpoint)
                    .await
                    .map_err(|_| SourceRejection::UnavailableEndpoint)?;
                validate_binding(&client, binding).await?;
                if !binding.capabilities.as_slice().iter().any(|entry| {
                    entry.name == ProviderCapabilityName::Load
                        && entry.status == ProviderCapabilityStatus::Supported
                }) {
                    return Err(SourceRejection::UnsupportedProviderAction);
                }
                let metadata = stored_load_metadata(&client, target).await?;
                let input = ConversationLoadInput {
                    operation_id: Some(operation_id.clone()),
                    target: target.clone(),
                    working_directory: PathBuf::from(String::from(metadata.working_directory)),
                    requested_by: requester,
                    approver: Some(metadata.approver),
                    access: metadata.requested_policy.access,
                    generation: Some(binding.generation.clone()),
                };
                match conversation.load(input, Duration::from_secs(10)).await {
                    Ok(result) => SourceProviderActionResult::ManagedLoad {
                        result: Box::new(result),
                    },
                    Err(ConversationClientError::OperationFailure(failure)) => {
                        SourceProviderActionResult::Failed {
                            operation_id: operation_id.clone(),
                            target: Some(target.clone()),
                            failure,
                        }
                    }
                    Err(_) => unknown_result(operation_id, Some(target)),
                }
            }
            SourceManagedAction::Resume { target } => {
                let inspected = client
                    .inspect_provider_session(ProviderSessionInspectRequest {
                        target: target.clone(),
                    })
                    .await
                    .map_err(|_| SourceRejection::SessionUnavailable)?;
                if inspected.target != *target || !inspected.capabilities.resume {
                    return Err(SourceRejection::UnsupportedProviderAction);
                }
                validate_binding(&client, binding).await?;
                let metadata = stored_load_metadata(&client, target).await?;
                let submitted = client
                    .resume_provider_conversation(ConversationResumeRequest {
                        operation_id: operation_id.clone(),
                        target: target.clone(),
                        generation: Some(binding.generation.clone()),
                        working_directory: metadata.working_directory,
                        requested_by: requester,
                        approver: metadata.approver,
                        requested_policy: metadata.requested_policy,
                    })
                    .await;
                match submitted {
                    Err(error) => failed_or_unknown(operation_id, target, error),
                    Ok(_) => match client
                        .wait_for_provider_conversation_operation(
                            ConversationOperationWaitRequest {
                                operation_id: operation_id.clone(),
                                timeout_seconds: PositiveSeconds::try_from(10u32)
                                    .map_err(|_| SourceRejection::InvalidRequest)?,
                            },
                        )
                        .await
                    {
                        Ok(result) if result.operation.target.as_ref() == Some(target) => {
                            SourceProviderActionResult::ManagedResume {
                                result: Box::new(result),
                            }
                        }
                        Ok(_) => unknown_result(operation_id, Some(target)),
                        Err(error) => failed_or_unknown(operation_id, target, error),
                    },
                }
            }
        };
        Ok(SourceReply::ProviderAction {
            result: Box::new(result),
        })
    };
    tokio::time::timeout(Duration::from_secs(30), operation)
        .await
        .unwrap_or_else(|_| {
            Ok(SourceReply::ProviderAction {
                result: Box::new(unknown_result(operation_id, target)),
            })
        })
}

fn unknown_result(
    operation_id: &OperationId,
    target: Option<&SessionRef>,
) -> SourceProviderActionResult {
    SourceProviderActionResult::Unknown {
        operation_id: operation_id.clone(),
        target: target.cloned(),
    }
}

fn failed_or_unknown(
    operation_id: &OperationId,
    target: &SessionRef,
    error: ClientError,
) -> SourceProviderActionResult {
    match error.conversation_failure() {
        Some(failure) => SourceProviderActionResult::Failed {
            operation_id: operation_id.clone(),
            target: Some(target.clone()),
            failure: Box::new(failure),
        },
        None => unknown_result(operation_id, Some(target)),
    }
}

async fn validate_binding(
    client: &CollaborationClient,
    expected: &ProviderBindingIdentity,
) -> Result<(), SourceRejection> {
    if expected.endpoint.service_id != client.identity().service_id {
        return Err(SourceRejection::WrongService);
    }
    let inventory = client
        .list_endpoints()
        .await
        .map_err(|_| SourceRejection::SourceUnavailable)?;
    if inventory.service_epoch != client.identity().service_epoch
        || inventory.service_epoch != expected.generation.service_epoch
    {
        return Err(SourceRejection::StaleBinding);
    }
    let mut endpoints = inventory
        .endpoints
        .iter()
        .filter(|entry| entry.endpoint == expected.endpoint);
    let entry = endpoints
        .next()
        .ok_or(SourceRejection::UnavailableEndpoint)?;
    if endpoints.next().is_some()
        || !matches!(entry.availability, EndpointAvailability::Available { .. })
    {
        return Err(SourceRejection::UnavailableEndpoint);
    }
    let mut bindings = entry.channels.iter().filter_map(|channel| match channel {
        ChannelDescription::ExternalProvider {
            transport,
            binding_id,
            binding_generation,
            runtime,
            capabilities,
        } => Some(ProviderBindingIdentity {
            endpoint: entry.endpoint.clone(),
            binding_id: binding_id.clone(),
            runtime: runtime.clone(),
            transport: *transport,
            generation: CodexGeneration {
                service_epoch: inventory.service_epoch.clone(),
                generation: *binding_generation,
            },
            capabilities: capabilities.clone(),
        }),
        _ => None,
    });
    let actual = bindings
        .next()
        .ok_or(SourceRejection::UnavailableEndpoint)?;
    if bindings.next().is_some() || actual != *expected {
        return Err(SourceRejection::StaleBinding);
    }
    Ok(())
}

async fn stored_load_metadata(
    client: &CollaborationClient,
    target: &SessionRef,
) -> Result<StoredLoadMetadata, SourceRejection> {
    let mut cursor = None;
    let mut seen_cursors = BTreeSet::new();
    let mut found = None;
    loop {
        let page = client
            .list_provider_sessions(ProviderSessionListParams {
                endpoint: target.endpoint.clone(),
                view: NativeSessionView::Stored,
                scope: NativeSessionScope::Any,
                source: NativeSessionSource::All,
                query: None,
                page_size: 100,
                cursor,
            })
            .await
            .map_err(|_| SourceRejection::SessionUnavailable)?;
        if page.endpoint != target.endpoint {
            return Err(SourceRejection::WrongService);
        }
        for summary in page.sessions {
            if summary.target() != target {
                continue;
            }
            let ProviderSessionSummary::HostedProvider {
                working_directory,
                requested_policy,
                approver,
                ..
            } = summary
            else {
                return Err(SourceRejection::SessionUnavailable);
            };
            if found.is_some() {
                return Err(SourceRejection::SessionUnavailable);
            }
            found = Some(StoredLoadMetadata {
                working_directory,
                requested_policy,
                approver,
            });
        }
        match page.next_cursor {
            Some(next) if seen_cursors.len() < 64 && seen_cursors.insert(next.clone()) => {
                cursor = Some(next)
            }
            Some(_) => return Err(SourceRejection::SessionUnavailable),
            None => break,
        }
    }
    found.ok_or(SourceRejection::SessionUnavailable)
}
