//! Provider creation consumes retained launch admission before dispatching ACP.
use super::provider_launch_admission::{ProviderCreateAdmission, launch_failure};
use super::{
    Arc, ConversationCreateRequest, ConversationOperationFailureKind,
    ConversationOperationFailureStage, ConversationOperationSettlement,
    ConversationOperationSubmission, ExternalProviderRuntimeError, ExternalProviderSupervisor,
    PathBuf, PreparedOperation, ProviderConversationFuture, ProviderOperationCompletion,
    ProviderOperationEffect, ProviderOperationKind, SessionId, SessionRef, admitted_submission,
    effective_settings, existing_submission, failure, invalid_setting_failure,
    provider_applied_setting, provider_failed_setting, provider_session_record, retain_operation,
    runtime_failure,
};

pub(super) fn create(
    supervisor: &ExternalProviderSupervisor,
    request: ConversationCreateRequest,
) -> ProviderConversationFuture<'_, ConversationOperationSubmission> {
    let operation_id = request.operation_id.clone();
    let backend = supervisor.clone();
    retain_operation(operation_id, None, async move {
        let (binding, runtime) =
            backend.runtime_binding(&request.endpoint, &request.operation_id, None)?;
        let prepared = backend
            .prepare_operation(
                request.operation_id.clone(),
                ProviderOperationKind::ConversationCreate,
                binding.clone(),
                None,
            )
            .await?;
        match prepared {
            PreparedOperation::Existing(operation) => Ok(existing_submission(operation)),
            PreparedOperation::Admitted { snapshot, live } => {
                let operation_id = request.operation_id.clone();
                let launch_admission = backend.inner.launch_admission.clone();
                let declared_request = request.clone();
                let requested_policy = request.requested_policy;
                let endpoint = binding.endpoint;
                let working_directory = request.working_directory;
                let created_by = request.created_by;
                let approver = request.approver;
                let requested_settings = request.settings.unwrap_or_default();
                let completion_inner = Arc::clone(&backend.inner);
                backend.spawn_operation(operation_id.clone(), live, async move {
                        let admission = match ProviderCreateAdmission::prepare(
                            &launch_admission, &declared_request, runtime.admission().supports_mcp_http,
                        ).await {
                            Ok(admission) => admission,
                            Err(error) => return ProviderOperationCompletion::Failure(*error),
                        };
                        let setup = match admission.client_setup().await {
                            Ok(setup) => setup,
                            Err(error) => return ProviderOperationCompletion::Failure(launch_failure(
                                operation_id, None, error,
                            )),
                        };
                        let result = runtime
                            .create_session_with_settings(
                                PathBuf::from(String::from(working_directory.clone())),
                                acp_client_runtime::RequestedProviderSettings {
                                    mode: requested_settings.mode,
                                    model: requested_settings.model,
                                    effort: requested_settings.effort,
                                }, setup)
                            .await;
                        match result {
                            Ok(created) => {
                                let catalog = runtime.settings_catalog(&created.provider_session_id).await;
                                match SessionId::try_from(created.provider_session_id) {
                                    Ok(session_id) => {
                                        let target = SessionRef {
                                            endpoint,
                                            session_id,
                                        };
                                        if let Some(catalog) = catalog {
                                            completion_inner.record_settings_catalog(target.clone(), catalog).await;
                                        }
                                        let mut observed = effective_settings(requested_policy.clone());
                                        observed.mode = created.effective_settings.mode;
                                        observed.model = created.effective_settings.model;
                                        observed.effort = created.effective_settings.effort;
                                        ProviderOperationCompletion::Success {
                                            settlement: ConversationOperationSettlement::Created {
                                                target: target.clone(),
                                                effective_settings: observed,
                                            },
                                            target: Some(target.clone()),
                                            session_record: Some(Box::new(
                                                provider_session_record(
                                                    target,
                                                    working_directory,
                                                    requested_policy,
                                                    created_by,
                                                    approver,
                                                ),
                                            )),
                                        }
                                    }
                                    Err(_) => ProviderOperationCompletion::Failure(failure(
                                        ConversationOperationFailureKind::ProtocolViolation,
                                        ConversationOperationFailureStage::Settlement,
                                        ProviderOperationEffect::Applied,
                                        "provider returned an invalid conversation identity",
                                        operation_id,
                                        None,
                                    )),
                                }
                            }
                            Err(ExternalProviderRuntimeError::CreatedWithoutSettings {
                                provider_session_id,
                                applied,
                                failed,
                                not_applied,
                            }) => {
                                let catalog = runtime.settings_catalog(&provider_session_id).await;
                                match SessionId::try_from(provider_session_id) {
                                Ok(session_id) => {
                                    let target = SessionRef { endpoint, session_id };
                                    if let Some(catalog) = catalog {
                                        completion_inner.record_settings_catalog(target.clone(), catalog).await;
                                    }
                                    ProviderOperationCompletion::Success {
                                        settlement: ConversationOperationSettlement::CreatedWithoutSettings {
                                            target: target.clone(),
                                            applied: applied.into_iter().map(provider_applied_setting).collect(),
                                            failed: vec![provider_failed_setting(failed)],
                                            not_applied: not_applied.into_iter().map(crate::provider_operation_settlement::provider_not_applied_setting).collect(),
                                        },
                                        target: Some(target.clone()),
                                        session_record: Some(Box::new(provider_session_record(
                                            target,
                                            working_directory,
                                            requested_policy,
                                            created_by,
                                            approver,
                                        ))),
                                    }
                                }
                                Err(_) => ProviderOperationCompletion::Failure(failure(
                                    ConversationOperationFailureKind::ProtocolViolation,
                                    ConversationOperationFailureStage::Settlement,
                                    ProviderOperationEffect::Applied,
                                    "provider returned an invalid conversation identity after partial settings",
                                    operation_id,
                                    None,
                                )),
                                }
                            }
                            Err(ExternalProviderRuntimeError::InvalidSetting {
                                setting,
                                value,
                                advertised,
                                provider_session_id,
                                disposition,
                            }) => {
                                let target = SessionId::try_from(provider_session_id)
                                    .ok()
                                    .map(|session_id| SessionRef { endpoint, session_id });
                                let failure = invalid_setting_failure(
                                    operation_id,
                                    target.clone(),
                                    setting,
                                    value,
                                    advertised,
                                    disposition,
                                );
                                match (target, disposition) {
                                    (Some(target), acp_client_runtime::InvalidSettingSessionDisposition::RemainsCreated) => {
                                        ProviderOperationCompletion::FailureWithSession {
                                            failure,
                                            session_record: Box::new(provider_session_record(
                                                target,
                                                working_directory,
                                                requested_policy,
                                                created_by,
                                                approver,
                                            )),
                                        }
                                    }
                                    _ => ProviderOperationCompletion::Failure(failure),
                                }
                            }
                            Err(ExternalProviderRuntimeError::SessionSetupFailed { provider_session_id, failure: setup_failure }) => {
                                let target = SessionId::try_from(provider_session_id.clone()).ok()
                                    .map(|session_id| SessionRef { endpoint, session_id });
                                ProviderOperationCompletion::Failure(runtime_failure(operation_id, target,
                                    ExternalProviderRuntimeError::SessionSetupFailed { provider_session_id, failure: setup_failure }))
                            }
                            Err(error) => {
                                let failure = runtime_failure(operation_id.clone(), None, error);
                                if failure.effect == ProviderOperationEffect::None
                                    && let Err(error) = admission.creation_refused().await
                                {
                                    return ProviderOperationCompletion::Failure(launch_failure(operation_id, None, error));
                                }
                                ProviderOperationCompletion::Failure(failure)
                            },
                        }
                    });
                Ok(admitted_submission(snapshot))
            }
        }
    })
}
