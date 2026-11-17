//! Register and settle concurrent provider Session admissions.
#[cfg(any(test, feature = "test-observation"))]
use super::ExternalProviderToolCall;

use super::provider_setting_application::{
    FAST_OFF, InitialSettingsOutcome, SettingSetupFailure, apply_initial_settings,
    fast_off_requested,
};
use super::{
    ActiveSession, Agent, Arc, CancellationToken, ConnectionTo, ExternalProviderCreatedSession,
    ExternalProviderRuntimeError, HashMap, InteractionPort, NewSessionRequest,
    PendingSessionAdmission, ProviderFrameObservation, ProviderSessionCommand,
    ProviderSessionRegistration, ProviderSessionRuntimeHandles, acp_operation_error,
    run_provider_session,
};
use crate::provider_item_projection::ProviderItemProjection;
use agent_client_protocol::schema::v1::CloseSessionRequest;

/// The requested settings after the failed one, in apply order. Fast counts
/// only when it would run, so a no-op default never becomes a hold (D1 R10).
fn untried_settings(
    requested: &crate::RequestedProviderSettings,
    catalog: &crate::ProviderSettingsCatalog,
    failed_kind: crate::ProviderSettingKind,
) -> Vec<crate::NotAppliedProviderSetting> {
    let fast_off = FAST_OFF.to_owned();
    let fast = fast_off_requested(requested.fast, catalog).then_some(&fast_off);
    let mut after_failure = false;
    let mut untried = Vec::new();
    for (kind, value) in [
        (crate::ProviderSettingKind::Mode, requested.mode.as_ref()),
        (crate::ProviderSettingKind::Model, requested.model.as_ref()),
        (
            crate::ProviderSettingKind::Effort,
            requested.effort.as_ref(),
        ),
        (crate::ProviderSettingKind::Fast, fast),
    ] {
        if after_failure && let Some(value) = value {
            untried.push(crate::NotAppliedProviderSetting {
                kind,
                value: value.clone(),
            });
        }
        if kind == failed_kind {
            after_failure = true;
        }
    }
    untried
}

pub(super) fn register_provider_session<P: InteractionPort>(
    registration: ProviderSessionRegistration<P>,
    sessions: &mut HashMap<String, tokio::sync::mpsc::Sender<ProviderSessionCommand<P>>>,
) -> Result<ExternalProviderCreatedSession, ExternalProviderRuntimeError> {
    if sessions.contains_key(&registration.provider_session_id) {
        return Err(ExternalProviderRuntimeError::Operation(
            "provider returned a duplicate conversation identity".to_owned(),
        ));
    }
    sessions.insert(
        registration.provider_session_id.clone(),
        registration.commands,
    );
    Ok(ExternalProviderCreatedSession {
        provider_session_id: registration.provider_session_id,
        effective_settings: registration.settings_catalog.effective_settings(),
        permission_mode: registration.permission_mode,
    })
}

pub(super) struct StaticSessionActivation {
    pub(super) session: ActiveSession<'static, Agent>,
    pub(super) item_projection: Option<ProviderItemProjection>,
    pub(super) activation: tokio::sync::oneshot::Receiver<()>,
}

pub(super) fn register_static_provider_session<P: InteractionPort>(
    admission: StaticSessionActivation,
    sessions: &mut HashMap<String, tokio::sync::mpsc::Sender<ProviderSessionCommand<P>>>,
    session_tasks: &mut tokio::task::JoinSet<()>,
    shutdown: CancellationToken,
    frame_observation: Arc<ProviderFrameObservation>,
    runtime_handles: ProviderSessionRuntimeHandles,
    #[cfg(any(test, feature = "test-observation"))] test_tool_calls: Arc<
        std::sync::Mutex<Vec<ExternalProviderToolCall>>,
    >,
) -> Result<ExternalProviderCreatedSession, ExternalProviderRuntimeError> {
    let StaticSessionActivation {
        session,
        item_projection,
        activation,
    } = admission;
    let provider_session_id = session.session_id().to_string();
    if sessions.contains_key(&provider_session_id) {
        return Err(ExternalProviderRuntimeError::Operation(
            "provider returned a duplicate conversation identity".to_owned(),
        ));
    }
    let (commands, command_rx) = tokio::sync::mpsc::channel(16);
    sessions.insert(provider_session_id.clone(), commands);
    session_tasks.spawn(async move {
        if activation.await.is_ok() {
            run_provider_session(
                session,
                command_rx,
                shutdown,
                frame_observation,
                runtime_handles,
                item_projection,
                #[cfg(any(test, feature = "test-observation"))]
                test_tool_calls,
            )
            .await;
        }
    });
    Ok(ExternalProviderCreatedSession {
        provider_session_id,
        effective_settings: crate::EffectiveProviderSettings::default(),
        permission_mode: crate::PermissionModeReport::default(),
    })
}

pub(super) struct CreateAdmissionInputs<P: InteractionPort> {
    pub(super) connection: ConnectionTo<Agent>,
    pub(super) request: NewSessionRequest,
    /// The session's in-folder roots, kept for its whole life; active while it
    /// runs in Router auto.
    pub(super) in_folder: Option<super::in_folder_allowance::InFolderGrant>,
    pub(super) requested_settings: crate::RequestedProviderSettings,
    pub(super) supports_close: bool,
    pub(super) shutdown: CancellationToken,
    pub(super) admission_tx: tokio::sync::mpsc::Sender<PendingSessionAdmission<P>>,
    pub(super) reply: tokio::sync::oneshot::Sender<super::CreateAdmissionReply>,
    pub(super) frame_observation: Arc<ProviderFrameObservation>,
    pub(super) runtime_handles: ProviderSessionRuntimeHandles,
    #[cfg(any(test, feature = "test-observation"))]
    pub(super) test_tool_calls: Arc<std::sync::Mutex<Vec<ExternalProviderToolCall>>>,
}

pub(super) async fn run_create_admission<P: InteractionPort>(inputs: CreateAdmissionInputs<P>) {
    let CreateAdmissionInputs {
        connection,
        request,
        in_folder,
        requested_settings,
        supports_close,
        shutdown,
        admission_tx,
        reply,
        frame_observation,
        runtime_handles,
        #[cfg(any(test, feature = "test-observation"))]
        test_tool_calls,
    } = inputs;
    let (registration_tx, mut registration_rx) = tokio::sync::oneshot::channel::<
        Result<ProviderSessionRegistration<P>, ExternalProviderRuntimeError>,
    >();
    let (activation_tx, activation_rx) = tokio::sync::oneshot::channel();
    let session_shutdown = shutdown.clone();
    let session_frame_observation = Arc::clone(&frame_observation);
    let session_future = connection
        .build_session_from(request)
        .block_task()
        .run_until(async move |session| {
            let provider_session_id = session.session_id().to_string();
            let response = session.response();
            let mut settings_catalog =
                crate::provider_settings_catalog_codec::catalog_from_session_response(&response, runtime_handles.behavior.as_ref(), &runtime_handles.connection_model_reasoning);
            // The caller's wish to keep fast off holds whatever this start's outcome (D1 R9).
            if fast_off_requested(requested_settings.fast, &settings_catalog) {
                runtime_handles.keep_fast_off.write().await.insert(provider_session_id.clone());
            }
            let InitialSettingsOutcome { applied, failure, permission_mode, router_auto_active } =
                apply_initial_settings(&session, &requested_settings, &mut settings_catalog, runtime_handles.behavior.as_ref()).await;
            if let Some(grant) = in_folder
                && let Ok(mut grants) = runtime_handles.in_folder_grants.write()
            {
                // The allowance follows the mode the agent confirmed (D1 R4).
                grants.insert(provider_session_id.clone(), super::in_folder_allowance::SessionInFolderGrant { grant, router_auto: router_auto_active });
            }
            let setup_error = match failure {
                None => None,
                Some(SettingSetupFailure::Invalid {
                    kind,
                    value,
                    advertised,
                }) => {
                    let disposition = if applied.is_empty() && supports_close {
                        let close = session
                            .connection()
                            .send_request_to(
                                Agent,
                                CloseSessionRequest::new(session.session_id().clone()),
                            )
                            .block_task()
                            .await;
                        if close.is_ok() {
                            crate::InvalidSettingSessionDisposition::Closed
                        } else {
                            crate::InvalidSettingSessionDisposition::RemainsCreated
                        }
                    } else {
                        crate::InvalidSettingSessionDisposition::RemainsCreated
                    };
                    let error = if applied.is_empty() {
                        ExternalProviderRuntimeError::InvalidSetting {
                            setting: kind,
                            value,
                            advertised,
                            provider_session_id: provider_session_id.clone(),
                            disposition,
                        }
                    } else {
                        ExternalProviderRuntimeError::CreatedWithoutSettings {
                            provider_session_id: provider_session_id.clone(),
                            not_applied: untried_settings(&requested_settings, &settings_catalog, kind).into_boxed_slice(),
                            applied: applied.into_boxed_slice(),
                            failed: crate::FailedProviderSetting {
                                kind,
                                value,
                                reason: session_event_model::ProviderSettingFailureReason::InvalidSetting,
                            },
                            permission_mode: Box::new(permission_mode.clone()),
                        }
                    };
                    if disposition == crate::InvalidSettingSessionDisposition::Closed {
                        // No Close admission follows a closed start, so its grant goes here.
                        if let Ok(mut grants) = runtime_handles.in_folder_grants.write() {
                            grants.remove(&provider_session_id);
                        }
                        runtime_handles.keep_fast_off.write().await.remove(&provider_session_id);
                        registration_tx
                            .send(Err(error))
                            .map_err(|_| agent_client_protocol::Error::internal_error())?;
                        return Ok(());
                    }
                    Some(error)
                }
                Some(SettingSetupFailure::Agent {
                    kind,
                    value,
                    reason,
                }) => Some(ExternalProviderRuntimeError::CreatedWithoutSettings {
                    provider_session_id: provider_session_id.clone(),
                    not_applied: untried_settings(&requested_settings, &settings_catalog, kind).into_boxed_slice(),
                    applied: applied.into_boxed_slice(),
                    failed: crate::FailedProviderSetting {
                        kind,
                        value,
                        reason: reason.into(),
                    },
                    permission_mode: Box::new(permission_mode.clone()),
                }),
                Some(SettingSetupFailure::Uncertain { kind, value }) => {
                    Some(ExternalProviderRuntimeError::CreatedWithoutSettings {
                        provider_session_id: provider_session_id.clone(),
                        not_applied: untried_settings(&requested_settings, &settings_catalog, kind).into_boxed_slice(),
                        applied: applied.into_boxed_slice(),
                        failed: crate::FailedProviderSetting {
                            kind,
                            value,
                            reason: session_event_model::ProviderSettingFailureReason::OutcomeUnknown,
                        },
                        permission_mode: Box::new(permission_mode.clone()),
                    })
                }
            };
            let (commands, command_rx) = tokio::sync::mpsc::channel(16);
            registration_tx
                .send(Ok(ProviderSessionRegistration {
                    provider_session_id,
                    commands,
                    response,
                    settings_catalog,
                    permission_mode,
                    setup_error,
                }))
                .map_err(|_| agent_client_protocol::Error::internal_error())?;
            // The connection opens the run's generation only when it admits
            // this identity, so a rejected duplicate opens nothing (D1 R14).
            let generation = activation_rx
                .await
                .map_err(|_| agent_client_protocol::Error::internal_error())?;
            run_provider_session(
                session,
                command_rx,
                session_shutdown,
                session_frame_observation,
                runtime_handles.for_session_generation(generation),
                None,
                #[cfg(any(test, feature = "test-observation"))]
                test_tool_calls,
            )
            .await;
            Ok(())
        });
    tokio::pin!(session_future);
    let result = tokio::select! {
        biased;
        () = shutdown.cancelled() => Err(ExternalProviderRuntimeError::TransportFailure),
        registration = &mut registration_rx => registration
            .map_err(|_| ExternalProviderRuntimeError::TransportFailure)
            .and_then(std::convert::identity),
        result = &mut session_future => match registration_rx.try_recv() {
            Ok(registration) => registration,
            Err(_) => Err(result
                .err()
                .map_or(ExternalProviderRuntimeError::TransportFailure, acp_operation_error)),
        },
    };
    let registered = result.is_ok();
    publish_pending_session_admission(
        &admission_tx,
        &shutdown,
        PendingSessionAdmission::Create {
            result: Box::new(result),
            activation: activation_tx,
            reply,
        },
    )
    .await;
    if registered {
        let _result = tokio::select! {
            () = shutdown.cancelled() => Ok(()),
            result = &mut session_future => result,
        };
    }
}

pub(super) async fn publish_pending_session_admission<P: InteractionPort>(
    admission_tx: &tokio::sync::mpsc::Sender<PendingSessionAdmission<P>>,
    shutdown: &CancellationToken,
    completion: PendingSessionAdmission<P>,
) {
    tokio::select! {
        biased;
        () = shutdown.cancelled() => fail_pending_session_admission(completion),
        permit = admission_tx.reserve() => match permit {
            Ok(permit) => permit.send(completion),
            Err(_) => fail_pending_session_admission(completion),
        },
    }
}

pub(super) fn fail_pending_session_admission<P: InteractionPort>(
    completion: PendingSessionAdmission<P>,
) {
    match completion {
        PendingSessionAdmission::Create { reply, .. } => {
            let _result = reply.send(super::CreateAdmissionReply {
                result: Err(ExternalProviderRuntimeError::TransportFailure),
                generation: None,
            });
        }
        PendingSessionAdmission::Restore { reply, .. } => {
            let _result = reply.send(Err(ExternalProviderRuntimeError::TransportFailure));
        }
        PendingSessionAdmission::Close { reply, .. } => {
            let _result = reply.send(Err(ExternalProviderRuntimeError::TransportFailure));
        }
    }
}

#[cfg(test)]
#[path = "provider_session_admission_test.rs"]
mod tests;
