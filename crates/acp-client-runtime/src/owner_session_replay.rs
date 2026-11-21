//! Decode load replay without publishing any part of it early.

use crate::provider_item_projection::{ItemProjectionError, ProviderItemProjection};
use crate::provider_settings_catalog_codec::apply_settings_update;
use crate::provider_update_kind::{is_known_update_kind, safe_update_kind};
use crate::{ExternalProviderRuntimeError, SessionEventSink};
use agent_client_protocol::{
    Dispatch,
    schema::v1::{SessionNotification, SessionUpdate},
};
use session_event_model::{InputId, SessionEvent, StopReason, TurnOutcome};
use std::sync::Arc;

pub(crate) fn replay_queued_session_updates(
    session_id: String,
    updates: &mut tokio::sync::mpsc::UnboundedReceiver<Dispatch>,
    event_sink: Arc<dyn SessionEventSink>,
    behavior: Arc<dyn crate::ProviderBehavior>,
    settings_catalog: &mut provider_session_machines::ProviderSettingsCatalog,
) -> Result<ProviderItemProjection, ExternalProviderRuntimeError> {
    let mut projection = ProviderItemProjection::new(
        session_id.clone(),
        Arc::clone(&event_sink),
        Arc::clone(&behavior),
    );
    projection.admit().map_err(replay_projection_error)?;
    let mut historical_turn: Option<String> = None;
    let mut saw_agent_output = false;
    while let Ok(update) = updates.try_recv() {
        let Dispatch::Notification(notification) = update else {
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
#[path = "owner_session_replay_test.rs"]
mod tests;
