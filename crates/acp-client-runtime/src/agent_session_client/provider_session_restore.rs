//! Restore one Session with or without agent history replay.

use super::{
    ActiveSession, Agent, Arc, CancellationToken, ConnectionTo, ExternalProviderRuntimeError,
    InteractionPort, LoadSessionRequest, McpServer, PathBuf, PendingSessionAdmission,
    RestoredProviderSession, SessionEventSink, acp_load_session_error, provider_frame_decode_error,
    publish_pending_session_admission,
};
use agent_client_protocol::schema::v1::{ResumeSessionRequest, SessionNotification, SessionUpdate};
use agent_client_protocol::{Dispatch, SessionMessage};
use futures_util::FutureExt as _;
use session_event_model::{InputId, SessionEvent, StopReason, TurnOutcome};
use std::{future::Future, time::Duration};

const RESTORE_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

/// Both restore entry points may lose their reply when the connection stops.
/// An orderly local shutdown retains the ordinary transport failure result.
pub(super) async fn complete_restore(
    completion: tokio::sync::oneshot::Receiver<
        Result<super::RestoredSessionAdmission, ExternalProviderRuntimeError>,
    >,
    local_shutdown: &CancellationToken,
    sink_closed: &CancellationToken,
) -> Result<super::RestoredSessionAdmission, ExternalProviderRuntimeError> {
    match completion.await {
        Ok(Err(ExternalProviderRuntimeError::RestoreDisconnected)) | Err(_)
            if local_shutdown.is_cancelled() =>
        {
            Err(ExternalProviderRuntimeError::TransportFailure)
        }
        Err(_) if sink_closed.is_cancelled() => Err(ExternalProviderRuntimeError::SinkClosed),
        Err(_) => Err(ExternalProviderRuntimeError::RestoreDisconnected),
        Ok(outcome) => outcome,
    }
}

async fn await_restore_response<T>(
    clock: &dyn router_clock::RuntimeClock,
    shutdown: &CancellationToken,
    request: impl Future<Output = Result<T, agent_client_protocol::Error>>,
) -> Result<T, ExternalProviderRuntimeError> {
    // Dropping the SDK's blocking restore future retires its provisional
    // route and cancels its correlation. This does not settle the peer's effect.
    tokio::select! {
        () = shutdown.cancelled() => Err(ExternalProviderRuntimeError::RestoreDisconnected),
        result = router_clock::timeout(clock, RESTORE_RESPONSE_TIMEOUT, request) => result
            .map_err(|_| ExternalProviderRuntimeError::RestoreNoAnswer)?
            .map_err(acp_load_session_error),
    }
}

use crate::provider_item_projection::{ItemProjectionError, ProviderItemProjection};
use crate::provider_settings_catalog_codec::{
    apply_settings_update, catalog_from_session_response,
};
use crate::provider_update_kind::{is_known_update_kind, safe_update_kind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RestoreHistoryMode {
    Replay,
    WithoutReplay,
}

pub(super) struct RestoreAdmissionInputs<P: InteractionPort> {
    pub(super) connection: ConnectionTo<Agent>,
    pub(super) provider_session_id: String,
    pub(super) cwd: PathBuf,
    pub(super) mode: RestoreHistoryMode,
    pub(super) mcp_servers: Vec<McpServer>,
    /// The session's folders and provider metadata, rebuilt as its create
    /// sent them (D1 R18; owner Decision 3).
    pub(super) folders: crate::session_start_folders::SessionStartFolders,
    pub(super) requested_mode: Option<String>,
    pub(super) shutdown: CancellationToken,
    pub(super) event_sink: Arc<dyn SessionEventSink>,
    pub(super) behavior: Arc<dyn crate::ProviderBehavior>,
    pub(super) connection_model_reasoning: crate::ModelReasoningMetadata,
    pub(super) admission_tx: tokio::sync::mpsc::Sender<PendingSessionAdmission<P>>,
    pub(super) generation: provider_session_machines::SessionGeneration,
    pub(super) attempt: provider_session_machines::session_lifecycle_machine::LoadAttempt,
    /// The connection's session cells: the restore's mode write is claimed
    /// on them for its load attempt (review F1).
    pub(super) session_cells: Arc<crate::session_cell_table::SessionCellTable>,
    pub(super) clock: Arc<dyn router_clock::RuntimeClock>,
    pub(super) reply: tokio::sync::oneshot::Sender<
        Result<super::RestoredSessionAdmission, ExternalProviderRuntimeError>,
    >,
}

pub(super) async fn run_restore_admission<P: InteractionPort>(inputs: RestoreAdmissionInputs<P>) {
    let RestoreAdmissionInputs {
        connection,
        provider_session_id,
        cwd,
        mode,
        mcp_servers,
        folders,
        requested_mode,
        shutdown,
        event_sink,
        behavior,
        connection_model_reasoning,
        admission_tx,
        generation,
        attempt,
        session_cells,
        clock,
        reply,
    } = inputs;
    let replay_started = if mode == RestoreHistoryMode::Replay {
        tokio::select! {
            () = shutdown.cancelled() => Err(ExternalProviderRuntimeError::TransportFailure),
            result = event_sink.begin_history_replay(&provider_session_id) =>
                result.map_err(|_| ExternalProviderRuntimeError::HistoryReplayAdmissionFailed),
        }
    } else {
        Ok(())
    };
    let result = match replay_started {
        Err(error) => Err(error),
        Ok(()) => match mode {
            RestoreHistoryMode::Replay => {
                let mut request = LoadSessionRequest::new(provider_session_id.clone(), &cwd)
                    .mcp_servers(mcp_servers)
                    .additional_directories(folders.additional_directories);
                request.meta = folders.meta;
                await_restore_response(
                    clock.as_ref(),
                    &shutdown,
                    connection
                        .load_session_from(request)
                        .block_task()
                        .start_session(),
                )
                .await
                .and_then(|restored| {
                    let mut session = restored.into_session();
                    let mut settings_catalog = catalog_from_session_response(
                        &session.response(),
                        behavior.as_ref(),
                        &connection_model_reasoning,
                    );
                    let projection = replay_queued_session_updates(
                        &mut session,
                        Arc::clone(&event_sink),
                        Arc::clone(&behavior),
                        &mut settings_catalog,
                    )?;
                    Ok((session, Some(projection), settings_catalog))
                })
            }
            RestoreHistoryMode::WithoutReplay => {
                let mut request = ResumeSessionRequest::new(provider_session_id.clone(), &cwd)
                    .mcp_servers(mcp_servers)
                    .additional_directories(folders.additional_directories);
                request.meta = folders.meta;
                await_restore_response(
                    clock.as_ref(),
                    &shutdown,
                    connection
                        .resume_session_from(request)
                        .block_task()
                        .start_session(),
                )
                .await
                .map(|restored| {
                    let session = restored.into_session();
                    let settings_catalog = catalog_from_session_response(
                        &session.response(),
                        behavior.as_ref(),
                        &connection_model_reasoning,
                    );
                    (session, None, settings_catalog)
                })
            }
        },
    };
    // The agent may restore the session in another mode: put back the mode
    // its record names before the session is registered (V6).
    let result = match result {
        Ok((session, item_projection, mut settings_catalog)) => {
            let restored_mode = crate::provider_setting_application::reapply_recorded_mode(
                &session,
                requested_mode.as_deref(),
                &mut settings_catalog,
                behavior.as_ref(),
                session_cells.read(&provider_session_id, |cell| {
                    cell.restore_mode_write.is_some()
                }),
                || {
                    crate::session_cell_bookkeeping::claim_restore_mode_write(
                        &session_cells,
                        &provider_session_id,
                        attempt,
                    )
                },
                clock.as_ref(),
            )
            .await;
            Ok(RestoredProviderSession {
                session,
                item_projection,
                settings_catalog,
                restored_mode,
            })
        }
        Err(error) => Err(error),
    };
    publish_pending_session_admission(
        &admission_tx,
        &shutdown,
        PendingSessionAdmission::Restore {
            generation,
            attempt,
            provider_session_id,
            result: Box::new(result),
            in_folder: folders.in_folder,
            reply,
        },
    )
    .await;
}

fn replay_queued_session_updates(
    session: &mut ActiveSession<'static, Agent>,
    event_sink: Arc<dyn SessionEventSink>,
    behavior: Arc<dyn crate::ProviderBehavior>,
    settings_catalog: &mut crate::ProviderSettingsCatalog,
) -> Result<ProviderItemProjection, ExternalProviderRuntimeError> {
    let session_id = session.session_id().to_string();
    let mut projection = ProviderItemProjection::new(
        session_id.clone(),
        Arc::clone(&event_sink),
        Arc::clone(&behavior),
    );
    projection.admit().map_err(replay_projection_error)?;
    let mut historical_turn: Option<String> = None;
    let mut saw_agent_output = false;
    while let Some(update) = session.read_update().now_or_never() {
        let update = update.map_err(provider_frame_decode_error)?;
        let SessionMessage::SessionMessage(Dispatch::Notification(notification)) = update else {
            continue;
        };
        if notification.method() != "session/update" {
            continue;
        }
        let params = notification.params();
        let kind = params
            .get("update")
            .and_then(|update| update.get("sessionUpdate"))
            .and_then(serde_json::Value::as_str);
        if let Some(kind) = kind
            && !is_known_update_kind(kind)
        {
            let source_kind = safe_update_kind(kind).unwrap_or("unrecognized");
            let content = params
                .get("update")
                .and_then(|update| update.get("content"))
                .and_then(|content| content.get("text"))
                .and_then(serde_json::Value::as_str);
            projection
                .observe_unknown(source_kind, content)
                .map_err(replay_projection_error)?;
            continue;
        }
        let Ok(notification) = serde_json::from_value::<SessionNotification>(params.clone()) else {
            tracing::warn!("malformed ACP replay update");
            continue;
        };
        apply_settings_update(settings_catalog, &notification.update, behavior.as_ref());
        let user_message = matches!(notification.update, SessionUpdate::UserMessageChunk(_));
        if user_message && (historical_turn.is_none() || saw_agent_output) {
            end_historical_turn(
                &mut historical_turn,
                &mut projection,
                &event_sink,
                &session_id,
            )?;
            let turn_id = uuid::Uuid::now_v7().to_string();
            event_sink
                .publish(
                    &session_id,
                    SessionEvent::TurnStarted {
                        turn_id: turn_id.clone(),
                        input_id: InputId::generate(),
                    },
                )
                .map_err(|_| ExternalProviderRuntimeError::SinkClosed)?;
            historical_turn = Some(turn_id);
            saw_agent_output = false;
        } else if !user_message {
            saw_agent_output = true;
        }
        projection
            .observe(&notification.update, params.get("update"))
            .map_err(replay_projection_error)?;
    }
    end_historical_turn(
        &mut historical_turn,
        &mut projection,
        &event_sink,
        &session_id,
    )?;
    Ok(projection)
}

fn end_historical_turn(
    turn_id: &mut Option<String>,
    projection: &mut ProviderItemProjection,
    event_sink: &Arc<dyn SessionEventSink>,
    session_id: &str,
) -> Result<(), ExternalProviderRuntimeError> {
    projection.finish().map_err(replay_projection_error)?;
    if let Some(turn_id) = turn_id.take() {
        event_sink
            .publish(
                session_id,
                SessionEvent::TurnEnded {
                    turn_id,
                    outcome: TurnOutcome::Ended {
                        stop_reason: StopReason::Unknown("replayed".to_owned()),
                        local_cause: None,
                    },
                },
            )
            .map_err(|_| ExternalProviderRuntimeError::SinkClosed)?;
    }
    Ok(())
}

fn replay_projection_error(error: ItemProjectionError) -> ExternalProviderRuntimeError {
    match error {
        ItemProjectionError::OutputLimit => ExternalProviderRuntimeError::HistoryReplayUnavailable,
        ItemProjectionError::SinkClosed => ExternalProviderRuntimeError::SinkClosed,
    }
}

#[cfg(test)]
#[path = "provider_session_restore_test.rs"]
mod tests;
