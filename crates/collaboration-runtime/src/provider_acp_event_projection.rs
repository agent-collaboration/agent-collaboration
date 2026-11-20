//! Projects shared provider Session events onto one ACP client connection.
use crate::provider_acp_session_route::failure;
use crate::provider_acp_session_route::response;
use collaboration_contracts::{HubEvent, SessionEventAttachment, SessionEventHub};
use message_board::SessionRef;
use serde_json::{Value, json};
use session_event_model::session_profile_codec::{ProfileState, ProfileTurn, StateNotification};
use session_event_model::{
    CapabilityReport, SessionEvent, SessionItemKind, SessionState, StopReason, ToolCallStatus,
    TurnLostReason, TurnOutcome,
};
use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};
use tokio::{sync::watch, task::JoinSet};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ObservedEventPosition {
    epoch: u64,
    sequence: u64,
}

const TURN_OUTCOME_UNREADABLE: &str = "the turn's outcome couldn't be read from the event stream; check the session before sending again";

fn capabilities_from_snapshot(
    snapshot: &[collaboration_contracts::HubEvent],
) -> Option<CapabilityReport> {
    snapshot
        .iter()
        .filter_map(|event| match &event.event {
            SessionEvent::CapabilitiesChanged { capabilities } => Some(capabilities.clone()),
            _ => None,
        })
        .next_back()
}

fn event_notification(
    event: &SessionEvent,
    session: &SessionRef,
    supports_state: bool,
    item_text: &mut HashMap<String, String>,
    active_turn: &mut Option<String>,
) -> Option<Value> {
    match event {
        SessionEvent::TurnStarted { turn_id, .. } => {
            *active_turn = Some(turn_id.clone());
            supports_state.then(|| {
                state_frame(
                    session,
                    ProfileState::Running,
                    Some(ProfileTurn::Running {
                        turn_id: turn_id.clone(),
                    }),
                )
            })
        }
        SessionEvent::TurnEnded { turn_id, outcome } => {
            let still_running = active_turn.as_ref().is_some_and(|active| active != turn_id);
            if active_turn.as_deref() == Some(turn_id) {
                *active_turn = None;
            }
            let (state, turn) = match outcome {
                TurnOutcome::Ended { stop_reason, .. } => {
                    let stop_reason = Some(stop_reason_label(stop_reason));
                    let turn = if matches!(stop_reason.as_deref(), Some("cancelled")) {
                        ProfileTurn::Interrupted {
                            turn_id: turn_id.clone(),
                            stop_reason,
                        }
                    } else {
                        ProfileTurn::Completed {
                            turn_id: turn_id.clone(),
                            stop_reason,
                        }
                    };
                    (ProfileState::Idle, turn)
                }
                TurnOutcome::Lost {
                    reason: TurnLostReason::EndNotObservable,
                } => (
                    ProfileState::Idle,
                    ProfileTurn::Lost {
                        turn_id: turn_id.clone(),
                        reason: "endNotObservable".into(),
                    },
                ),
                TurnOutcome::Lost { reason } => (
                    ProfileState::Unloaded,
                    ProfileTurn::Lost {
                        turn_id: turn_id.clone(),
                        reason: match reason {
                            TurnLostReason::ProviderRetired => "providerRetired",
                            TurnLostReason::ProviderTurnFailed => "providerTurnFailed",
                            TurnLostReason::EndNotObservable => "endNotObservable",
                        }
                        .into(),
                    },
                ),
            };
            let state = if still_running {
                ProfileState::Running
            } else {
                state
            };
            supports_state.then(|| state_frame(session, state, Some(turn)))
        }
        SessionEvent::StateChanged { state } if supports_state => {
            let state = match state {
                SessionState::Unloaded => Some(ProfileState::Unloaded),
                SessionState::Running => Some(ProfileState::Running),
                SessionState::Idle => Some(ProfileState::Idle),
                SessionState::RequiresAction { pending } => Some(ProfileState::RequiresAction {
                    kind: pending.first().kind(),
                }),
                SessionState::AuthenticationRequired => Some(ProfileState::AuthenticationRequired),
                SessionState::Closed => Some(ProfileState::Closed),
            }?;
            let turn = active_turn.as_ref().map(|turn_id| ProfileTurn::Running {
                turn_id: turn_id.clone(),
            });
            Some(state_frame(session, state, turn))
        }
        SessionEvent::ItemStarted { item } | SessionEvent::ItemUpdated { item }
            if matches!(item.kind, SessionItemKind::ToolCall { .. }) =>
        {
            let SessionItemKind::ToolCall { tool_kind, status } = &item.kind else {
                return None;
            };
            let kind = match tool_kind.as_str() {
                "read" | "edit" | "delete" | "move" | "search" | "execute" | "think" | "fetch"
                | "switch_mode" => tool_kind.as_str(),
                _ => "other",
            };
            let status = match status {
                ToolCallStatus::Pending => "pending",
                ToolCallStatus::InProgress => "in_progress",
                ToolCallStatus::Completed => "completed",
                ToolCallStatus::Failed => "failed",
            };
            let started = matches!(event, SessionEvent::ItemStarted { .. });
            let update = if started {
                json!({"sessionUpdate":"tool_call","toolCallId":item.item_id,
                    "kind":kind,"status":status,
                    "title":item.text.as_deref().unwrap_or(tool_kind)})
            } else {
                json!({"sessionUpdate":"tool_call_update","toolCallId":item.item_id,
                    "status":status,"title":item.text})
            };
            Some(json!({"jsonrpc":"2.0","method":"session/update","params":{
                "sessionId":session.session_id.as_str(),"update":update
            }}))
        }
        SessionEvent::ItemStarted { item } | SessionEvent::ItemUpdated { item } => {
            let text = item.text.as_deref()?;
            let previous = item_text.insert(item.item_id.clone(), text.to_owned());
            let delta = previous
                .as_ref()
                .and_then(|before| text.strip_prefix(before))
                .unwrap_or(text);
            if delta.is_empty() {
                return None;
            }
            let update_kind = match item.kind {
                SessionItemKind::AgentThought => "agent_thought_chunk",
                SessionItemKind::UserMessage => "user_message_chunk",
                _ => "agent_message_chunk",
            };
            Some(json!({"jsonrpc":"2.0","method":"session/update","params":{
                "sessionId":session.session_id.as_str(),
                "update":{"sessionUpdate":update_kind,"content":{"type":"text","text":delta}}
            }}))
        }
        _ => None,
    }
}

fn state_frame(session: &SessionRef, state: ProfileState, turn: Option<ProfileTurn>) -> Value {
    let notification = StateNotification {
        session_id: session.session_id.as_str().to_owned(),
        state,
        turn,
    };
    json!({"jsonrpc":"2.0","method":notification.method(),"params":notification})
}

fn stop_reason_label(reason: &StopReason) -> String {
    match reason {
        StopReason::EndTurn => "end_turn".into(),
        StopReason::MaxTokens => "max_tokens".into(),
        StopReason::MaxTurnRequests => "max_turn_requests".into(),
        StopReason::Refusal => "refusal".into(),
        StopReason::Cancelled => "cancelled".into(),
        StopReason::Unknown(value) => value.clone(),
    }
}

pub(crate) struct ProviderSessionObservers {
    events: Arc<dyn SessionEventHub>,
    output: codex_acp_adapter::AcpOutputSender,
    supports_state: bool,
    pub(crate) tasks: JoinSet<()>,
    attached: BTreeSet<SessionRef>,
    pub(crate) observed_events: HashMap<SessionRef, watch::Receiver<Option<ObservedEventPosition>>>,
    cancellations: HashMap<SessionRef, CancellationToken>,
}

impl ProviderSessionObservers {
    pub(crate) fn new(
        events: Arc<dyn SessionEventHub>,
        output: codex_acp_adapter::AcpOutputSender,
        supports_state: bool,
    ) -> Self {
        Self {
            events,
            output,
            supports_state,
            tasks: JoinSet::new(),
            attached: BTreeSet::new(),
            observed_events: HashMap::new(),
            cancellations: HashMap::new(),
        }
    }

    pub(crate) async fn attach(
        &mut self,
        session: SessionRef,
        replay_history: bool,
        replace_existing: bool,
    ) -> Result<CapabilityReport, ()> {
        if replace_existing {
            if let Some(previous) = self.cancellations.remove(&session) {
                previous.cancel();
            }
            self.attached.remove(&session);
            self.observed_events.remove(&session);
        }
        let attachment = self.events.attach(session.clone()).await.map_err(|_| ())?;
        let report = capabilities_from_snapshot(&attachment.snapshot).ok_or(())?;
        if self.attached.insert(session.clone()) {
            let (progress, observed) = watch::channel(None);
            self.observed_events.insert(session.clone(), observed);
            let cancellation = CancellationToken::new();
            self.cancellations
                .insert(session.clone(), cancellation.clone());
            let mut item_text = HashMap::new();
            let mut active_turn = None;
            if replay_history {
                for event in &attachment.snapshot {
                    if let Some(notification) = event_notification(
                        &event.event,
                        &session,
                        self.supports_state,
                        &mut item_text,
                        &mut active_turn,
                    ) {
                        self.output.send(notification).await.map_err(|_| ())?;
                    }
                }
            }
            let mut epoch = attachment.epoch;
            if let Some(last) = attachment.snapshot.last() {
                progress.send_replace(Some(ObservedEventPosition {
                    epoch,
                    sequence: last.sequence,
                }));
            }
            let mut receiver = attachment.receiver;
            let output = self.output.clone();
            let events = Arc::clone(&self.events);
            let supports_state = self.supports_state;
            self.tasks.spawn(async move {
                loop {
                    let received = tokio::select! {
                        biased;
                        () = cancellation.cancelled() => break,
                        received = receiver.recv() => received,
                    };
                    let event = match received {
                        Ok(event)
                            if !matches!(event.event, SessionEvent::ResyncRequired { .. }) =>
                        {
                            Some(event)
                        }
                        Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => None,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    };
                    if cancellation.is_cancelled() {
                        break;
                    }
                    let Some(event) = event else {
                        let Ok(replacement) = events.attach(session.clone()).await else {
                            break;
                        };
                        item_text.clear();
                        active_turn = None;
                        for historical in &replacement.snapshot {
                            if let Some(notification) = event_notification(
                                &historical.event,
                                &session,
                                supports_state,
                                &mut item_text,
                                &mut active_turn,
                            ) && output.send(notification).await.is_err()
                            {
                                return;
                            }
                        }
                        epoch = replacement.epoch;
                        if let Some(last) = replacement.snapshot.last() {
                            progress.send_replace(Some(ObservedEventPosition {
                                epoch,
                                sequence: last.sequence,
                            }));
                        }
                        receiver = replacement.receiver;
                        continue;
                    };
                    if let Some(notification) = event_notification(
                        &event.event,
                        &session,
                        supports_state,
                        &mut item_text,
                        &mut active_turn,
                    ) && output.send(notification).await.is_err()
                    {
                        break;
                    }
                    progress.send_replace(Some(ObservedEventPosition {
                        epoch,
                        sequence: event.sequence,
                    }));
                }
            });
        }
        Ok(report)
    }
}

pub(crate) async fn stream_prompt(
    output: codex_acp_adapter::AcpOutputSender,
    id: Value,
    turn_id: String,
    events: &dyn SessionEventHub,
    session: SessionRef,
    mut attachment: SessionEventAttachment,
    mut observed: watch::Receiver<Option<ObservedEventPosition>>,
) {
    use tokio::sync::broadcast::error::RecvError;

    let mut replay = std::mem::take(&mut attachment.snapshot).into_iter();
    loop {
        let received = match replay.next() {
            Some(event) => Ok(event),
            None => attachment.receiver.recv().await,
        };
        let event = match received {
            Ok(HubEvent {
                event: SessionEvent::ResyncRequired { .. },
                ..
            })
            | Err(RecvError::Lagged(_)) => {
                let resync = received.is_ok();
                let replacement = events.attach(session.clone()).await;
                let Ok(replacement) = replacement else {
                    let _sent = output
                        .send(failure(id, -32000, TURN_OUTCOME_UNREADABLE))
                        .await;
                    return;
                };
                if (resync || replacement.epoch != attachment.epoch)
                    && !replacement.snapshot.iter().any(|event| matches!(
                        &event.event, SessionEvent::TurnEnded { turn_id: ended, .. } if ended == &turn_id
                    ))
                {
                    let _sent = output.send(failure(id, -32000, TURN_OUTCOME_UNREADABLE)).await;
                    return;
                }
                attachment = replacement;
                replay = std::mem::take(&mut attachment.snapshot).into_iter();
                continue;
            }
            Ok(event) => event,
            Err(RecvError::Closed) => {
                let _sent = output
                    .send(failure(id, -32000, "Session event stream unavailable"))
                    .await;
                return;
            }
        };
        if let SessionEvent::TurnEnded {
            turn_id: ended,
            outcome,
        } = event.event
            && ended == turn_id
        {
            let position = ObservedEventPosition {
                epoch: attachment.epoch,
                sequence: event.sequence,
            };
            let frame = if observed
                .wait_for(|observed| observed.is_some_and(|observed| observed >= position))
                .await
                .is_err()
            {
                failure(id, -32000, "Session observer unavailable")
            } else {
                match outcome {
                    TurnOutcome::Ended {
                        stop_reason: StopReason::Unknown(_),
                        ..
                    } => failure(id, -32000, TURN_OUTCOME_UNREADABLE),
                    TurnOutcome::Ended { stop_reason, .. } => {
                        response(id, json!({"stopReason":stop_reason_label(&stop_reason)}))
                    }
                    TurnOutcome::Lost { .. } => failure(id, -32000, "Session turn lost"),
                }
            };
            let _sent = output.send(frame).await;
            return;
        }
    }
}

#[cfg(test)]
#[path = "provider_acp_prompt_waiter_test.rs"]
mod prompt_waiter_tests;

#[cfg(test)]
mod state_tests {
    use super::*;

    fn session() -> SessionRef {
        serde_json::from_value(json!({
            "endpoint":{"serviceId":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89","endpointId":"claude-local"},
            "sessionId":"provider-session"
        }))
        .expect("session")
    }

    #[test]
    fn turn_state_is_sent_only_to_clients_that_advertised_state() {
        let session = session();
        let mut item_text = HashMap::new();
        let mut active_turn = None;
        let started = SessionEvent::TurnStarted {
            turn_id: "turn-1".into(),
            input_id: session_event_model::InputId::generate(),
        };
        assert!(
            event_notification(&started, &session, false, &mut item_text, &mut active_turn)
                .is_none()
        );
        assert_eq!(active_turn.as_deref(), Some("turn-1"));
        let started_frame =
            event_notification(&started, &session, true, &mut item_text, &mut active_turn)
                .expect("state notification");
        assert_eq!(
            started_frame["params"]["turn"],
            json!({"status":"running","turnId":"turn-1"})
        );
        let ended = SessionEvent::TurnEnded {
            turn_id: "turn-1".into(),
            outcome: TurnOutcome::Ended {
                stop_reason: StopReason::Cancelled,
                local_cause: None,
            },
        };
        let ended_frame =
            event_notification(&ended, &session, true, &mut item_text, &mut active_turn)
                .expect("state notification");
        assert_eq!(ended_frame["params"]["state"], "idle");
        assert_eq!(
            ended_frame["params"]["turn"],
            json!({"status":"interrupted","turnId":"turn-1","stopReason":"cancelled"})
        );
        assert!(active_turn.is_none());

        let _started_again =
            event_notification(&started, &session, true, &mut item_text, &mut active_turn);
        let not_observable = SessionEvent::TurnEnded {
            turn_id: "turn-1".into(),
            outcome: TurnOutcome::Lost {
                reason: TurnLostReason::EndNotObservable,
            },
        };
        let lost_frame = event_notification(
            &not_observable,
            &session,
            true,
            &mut item_text,
            &mut active_turn,
        )
        .expect("lost turn state");
        assert_eq!(lost_frame["params"]["state"], "idle");
        assert_eq!(
            lost_frame["params"]["turn"],
            json!({"status":"lost","turnId":"turn-1","reason":"endNotObservable"})
        );
    }
}
