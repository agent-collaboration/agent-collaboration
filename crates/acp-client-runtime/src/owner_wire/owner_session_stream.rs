//! Decode SDK session updates into stamped owner facts.

use crate::{
    EventSinkClosed, ExternalProviderRuntimeError, HistoryReplayFuture, ProviderBehavior,
    ProviderCapabilityReport, ProviderSettingsCatalog, SessionEventSink,
};
use crate::{
    acp_transport::TransportRun,
    owner_session_open::{CreatedStream, OpenedSessionRoute, OwnerOpenedSession},
    owner_session_replay::replay_queued_session_updates,
    owner_wire::{OwnerAcpWire, PromptTerminal},
    provider_item_projection::ProviderItemProjection,
    provider_settings_catalog_codec::apply_settings_update,
    provider_update_kind::{is_known_update_kind, safe_update_kind},
    session_owner_task::OwnerCompletionPort,
};
use agent_client_protocol::{
    ActiveSession, Agent, Dispatch, SessionMessage,
    schema::v1::{ContentBlock, SessionNotification, SessionUpdate},
};
use futures_util::FutureExt as _;
use provider_session_machines::session_owner::{OwnerAddress, OwnerEvent};
use provider_session_machines::session_turn_machine::TurnId;
use session_event_model::session_profile_codec::{ProfileState, STATE_METHOD, StateNotification};
use session_event_model::{LocalCause, SessionEvent, SessionSettings, SkillView};
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[cfg(any(test, feature = "test-observation"))]
pub(crate) type TestToolCallRecords =
    Arc<Mutex<Vec<crate::provider_client_contract::ExternalProviderToolCall>>>;

enum StreamFact {
    Event(SessionEvent),
    Skills(SkillView),
}

#[derive(Default)]
pub(crate) struct ProjectionBuffer(Mutex<Vec<StreamFact>>);

impl ProjectionBuffer {
    fn facts(&self) -> MutexGuard<'_, Vec<StreamFact>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn take(&self) -> Vec<StreamFact> {
        std::mem::take(&mut self.facts())
    }
}

impl SessionEventSink for ProjectionBuffer {
    fn publish_skill_view(&self, _: &str, view: SkillView) -> Result<(), EventSinkClosed> {
        self.facts().push(StreamFact::Skills(view));
        Ok(())
    }

    fn begin_history_replay(&self, _: &str) -> HistoryReplayFuture<'_> {
        Box::pin(std::future::ready(Ok(())))
    }

    fn publish(&self, _: &str, event: SessionEvent) -> Result<(), EventSinkClosed> {
        self.facts().push(StreamFact::Event(event));
        Ok(())
    }
}

pub struct ReplayedSessionFacts {
    pub events: Vec<SessionEvent>,
    pub skills: Option<SkillView>,
}

pub struct OwnerStreamTarget {
    pub owner: OwnerAddress,
    pub run: TransportRun,
    pub wire: Arc<OwnerAcpWire>,
    pub completions: OwnerCompletionPort,
    pub retired: CancellationToken,
    terminals: mpsc::Receiver<PromptTerminal>,
}

impl OwnerStreamTarget {
    pub fn new(
        owner: OwnerAddress,
        run: TransportRun,
        wire: Arc<OwnerAcpWire>,
        completions: OwnerCompletionPort,
        retired: CancellationToken,
    ) -> Self {
        let terminals = wire.register_reader(run);
        Self {
            owner,
            run,
            wire,
            completions,
            retired,
            terminals,
        }
    }

    async fn flush_facts(&self, buffer: &ProjectionBuffer, turn: Option<TurnId>) -> bool {
        for fact in buffer.take() {
            let event = match fact {
                StreamFact::Event(event) => OwnerEvent::Stream {
                    run: self.run.generation,
                    expected_turn: turn,
                    event,
                },
                StreamFact::Skills(view) => OwnerEvent::SkillView {
                    run: self.run.generation,
                    view,
                },
            };
            if self
                .completions
                .send_stream(self.owner, event, &self.retired)
                .await
                .is_err()
            {
                return false;
            }
        }
        true
    }
}

#[derive(Default)]
struct TurnResponse {
    text: String,
    overflow: bool,
    #[cfg(any(test, feature = "test-observation"))]
    tool_calls: std::collections::BTreeMap<
        String,
        crate::provider_client_contract::ExternalProviderToolCall,
    >,
}

#[derive(Default)]
struct CapturedTurnResponses {
    projecting: Option<TurnId>,
    turns: std::collections::BTreeMap<TurnId, TurnResponse>,
}

enum SessionUpdates<'borrow, 'runner> {
    Created(&'borrow mut ActiveSession<'runner, Agent>),
    Restored(mpsc::UnboundedReceiver<Dispatch>),
}

impl SessionUpdates<'_, '_> {
    async fn read(&mut self) -> Result<SessionMessage, ExternalProviderRuntimeError> {
        match self {
            Self::Created(sdk) => sdk
                .read_update()
                .await
                .map_err(crate::provider_client_contract::provider_frame_decode_error),
            Self::Restored(updates) => updates
                .recv()
                .await
                .map(SessionMessage::SessionMessage)
                .ok_or(ExternalProviderRuntimeError::TransportFailure),
        }
    }
}

impl TurnResponse {
    fn append(&mut self, text: &str) -> bool {
        if self.overflow {
            return false;
        }
        let remaining = crate::provider_client_contract::MAX_PROMPT_OUTPUT_BYTES
            .saturating_sub(self.text.len());
        let end = text.floor_char_boundary(remaining.min(text.len()));
        self.text.push_str(text.get(..end).unwrap_or_default());
        self.overflow = end != text.len();
        self.overflow
    }

    fn into_response(self) -> (String, Option<LocalCause>) {
        (
            self.text,
            self.overflow.then_some(LocalCause::OutputOverflow),
        )
    }

    #[cfg(any(test, feature = "test-observation"))]
    fn observe_tool(&mut self, update: &SessionUpdate) {
        use crate::provider_client_contract::{
            ExternalProviderToolCall, ExternalProviderToolOutcome, classify_mcp_tool_outcome,
        };
        match update {
            SessionUpdate::ToolCall(tool) => {
                self.tool_calls.insert(
                    tool.tool_call_id.0.to_string(),
                    ExternalProviderToolCall {
                        name: tool.name.clone(),
                        title: tool.title.clone(),
                        kind: tool.kind,
                        status: tool.status,
                        outcome: classify_mcp_tool_outcome(tool.raw_output.as_ref()),
                    },
                );
            }
            SessionUpdate::ToolCallUpdate(update) => {
                if let Some(tool) = self.tool_calls.get_mut(update.tool_call_id.0.as_ref()) {
                    let fields = &update.fields;
                    if let Some(name) = &fields.name {
                        tool.name = Some(name.clone());
                    }
                    if let Some(title) = &fields.title {
                        tool.title.clone_from(title);
                    }
                    tool.kind = fields.kind.unwrap_or(tool.kind);
                    tool.status = fields.status.unwrap_or(tool.status);
                    let outcome = classify_mcp_tool_outcome(fields.raw_output.as_ref());
                    if outcome != ExternalProviderToolOutcome::Unknown {
                        tool.outcome = outcome;
                    }
                }
            }
            _ => {}
        }
    }
}

impl OwnerOpenedSession {
    #[must_use]
    pub fn capabilities(&self) -> &ProviderCapabilityReport {
        &self.capabilities
    }

    #[must_use]
    pub fn catalog(&self) -> &ProviderSettingsCatalog {
        &self.catalog
    }

    #[must_use]
    pub fn settings(&self) -> SessionSettings {
        self.catalog.to_session_settings()
    }

    pub fn prepare_replay(&mut self) -> Result<ReplayedSessionFacts, ExternalProviderRuntimeError> {
        let session_id = self.provider_id();
        let Some(OpenedSessionRoute::Restored { updates, .. }) = self.route.as_mut() else {
            return Err(ExternalProviderRuntimeError::HistoryReplayUnavailable);
        };
        self.projection = Some(replay_queued_session_updates(
            session_id,
            updates,
            Arc::clone(&self.buffer) as Arc<dyn SessionEventSink>,
            Arc::clone(&self.behavior),
            &mut self.catalog,
        )?);
        let mut result = ReplayedSessionFacts {
            events: Vec::new(),
            skills: None,
        };
        for fact in self.buffer.take() {
            match fact {
                StreamFact::Event(event) => result.events.push(event),
                StreamFact::Skills(view) => result.skills = Some(view),
            }
        }
        Ok(result)
    }

    /// Consume the SDK route as a stream executor. Scope and presentation
    /// changes are admitted by the owner against the captured run and turn.
    pub async fn read_stream(
        mut self,
        target: OwnerStreamTarget,
    ) -> Result<(), ExternalProviderRuntimeError> {
        match self.route.take() {
            Some(OpenedSessionRoute::Restored { sdk, updates }) => {
                let result = self
                    .read_updates(SessionUpdates::Restored(updates), target)
                    .await;
                drop(sdk);
                result
            }
            Some(OpenedSessionRoute::Created(activate)) => {
                let (finished, result) = tokio::sync::oneshot::channel();
                activate
                    .send(CreatedStream {
                        opened: self,
                        target,
                        finished,
                    })
                    .map_err(|_| ExternalProviderRuntimeError::TransportFailure)?;
                result
                    .await
                    .map_err(|_| ExternalProviderRuntimeError::TransportFailure)?
            }
            None => Err(ExternalProviderRuntimeError::TransportFailure),
        }
    }

    pub(crate) async fn read_sdk_stream(
        self,
        sdk: &mut ActiveSession<'_, Agent>,
        target: OwnerStreamTarget,
    ) -> Result<(), ExternalProviderRuntimeError> {
        self.read_updates(SessionUpdates::Created(sdk), target)
            .await
    }

    async fn read_updates(
        mut self,
        mut updates: SessionUpdates<'_, '_>,
        mut target: OwnerStreamTarget,
    ) -> Result<(), ExternalProviderRuntimeError> {
        let mut projection = self.projection.take().unwrap_or_else(|| {
            ProviderItemProjection::new(
                self.provider_id(),
                Arc::clone(&self.buffer) as Arc<dyn SessionEventSink>,
                Arc::clone(&self.behavior),
            )
        });
        let mut captured = CapturedTurnResponses::default();
        loop {
            tokio::select! {
                biased;
                () = target.retired.cancelled() => return Ok(()),
                result = updates.read() => {
                    let update = result?;
                    if !self.deliver_update(update, &target, &mut projection, &mut captured).await? {
                        return Ok(());
                    }
                }
                Some(terminal) = target.terminals.recv() => {
                    while let Some(ready) = updates.read().now_or_never() {
                        let update = ready?;
                        if !self.deliver_update(update, &target, &mut projection, &mut captured).await? {
                            return Ok(());
                        }
                    }
                    let stop_reason = terminal.stop_reason();
                    let mut compact_unconfirmed = false;
                    let was_current = target.wire.current_turn(target.run) == Some(terminal.turn);
                    let turn_response = captured.turns.remove(&terminal.turn).unwrap_or_default();
                    if was_current {
                        #[cfg(any(test, feature = "test-observation"))]
                        {
                            *self.observed_tool_calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
                                turn_response.tool_calls.values().cloned().collect();
                        }
                        if terminal.compact && let Some(stop) = stop_reason.as_ref()
                            && *stop != session_event_model::StopReason::Cancelled
                        {
                            compact_unconfirmed = *stop != session_event_model::StopReason::EndTurn
                                || !projection.compact_confirmed();
                            if !compact_unconfirmed {
                                projection.promote_compaction()
                                    .map_err(|_| ExternalProviderRuntimeError::PromptOutputLimitExceeded)?;
                            }
                        }
                        if stop_reason.is_some() {
                            let _output_limited = projection.finish();
                        }
                        if !target.flush_facts(&self.buffer, Some(terminal.turn)).await {
                            return Ok(());
                        }
                    }
                    let (text, cause) = turn_response.into_response();
                    let cause = cause.or_else(|| {
                        compact_unconfirmed.then(|| LocalCause::Other("compactionUnconfirmed".to_owned()))
                    });
                    if was_current && stop_reason.is_none() {
                        projection.discard_turn_local_cursors();
                    }
                    terminal.finish(text, cause);
                }
            }
        }
    }

    async fn deliver_update(
        &mut self,
        update: SessionMessage,
        target: &OwnerStreamTarget,
        projection: &mut ProviderItemProjection,
        captured: &mut CapturedTurnResponses,
    ) -> Result<bool, ExternalProviderRuntimeError> {
        if let SessionMessage::SessionMessage(Dispatch::Notification(notification)) = &update
            && notification.method() == STATE_METHOD
        {
            if let Some(turn) = target.wire.provider_started_turn(target.run)
                && let Ok(state) =
                    serde_json::from_value::<StateNotification>(notification.params().clone())
                && state.session_id == self.provider_id()
                && state.state == ProfileState::Idle
            {
                let _flushed = projection.finish();
                captured.turns.remove(&turn);
                if !target.flush_facts(&self.buffer, Some(turn)).await {
                    return Ok(false);
                }
                let end_seen = OwnerEvent::ObservedTurnEnded {
                    run: target.run.generation,
                    turn,
                    outcome: session_event_model::TurnOutcome::Ended {
                        stop_reason: session_event_model::StopReason::Unknown(
                            "provider_idle".to_owned(),
                        ),
                        local_cause: None,
                    },
                };
                return Ok(target
                    .completions
                    .send_stream(target.owner, end_seen, &target.retired)
                    .await
                    .is_ok());
            }
            return Ok(true);
        }
        let turn = target.wire.current_turn(target.run);
        if let Some(turn) = turn
            && captured.projecting != Some(turn)
        {
            captured.projecting = Some(turn);
            projection.discard_turn_local_cursors();
        }
        let mut current = turn.map(|turn| captured.turns.entry(turn).or_default());
        let (text, settings_updated) = project_update(
            update,
            projection,
            &mut self.catalog,
            self.behavior.as_ref(),
            &self.buffer,
            #[cfg(any(test, feature = "test-observation"))]
            |update| {
                if let Some(response) = current.as_deref_mut() {
                    response.observe_tool(update);
                }
            },
        )?;
        if settings_updated {
            target.wire.update_catalog(target.run, &self.catalog);
        }
        if let (Some(turn), Some(text), Some(response)) = (turn, text, current.take())
            && response.append(&text)
            && target
                .completions
                .send_stream(
                    target.owner,
                    OwnerEvent::OutputLimit {
                        run: target.run.generation,
                        turn,
                    },
                    &target.retired,
                )
                .await
                .is_err()
        {
            return Ok(false);
        }
        Ok(target.flush_facts(&self.buffer, turn).await)
    }
}

fn project_update(
    update: SessionMessage,
    projection: &mut ProviderItemProjection,
    catalog: &mut ProviderSettingsCatalog,
    behavior: &dyn ProviderBehavior,
    buffer: &ProjectionBuffer,
    #[cfg(any(test, feature = "test-observation"))] observe_tool: impl FnOnce(&SessionUpdate),
) -> Result<(Option<String>, bool), ExternalProviderRuntimeError> {
    let SessionMessage::SessionMessage(dispatch) = update else {
        return Ok((None, false));
    };
    let Dispatch::Notification(notification) = &dispatch else {
        return Ok((None, false));
    };
    if notification.method() != "session/update" {
        return Ok((None, false));
    }
    let params = notification.params();
    let raw_update = params.get("update");
    let kind = raw_update
        .and_then(|raw| raw.get("sessionUpdate"))
        .and_then(serde_json::Value::as_str);
    if let Some(kind) = kind
        && !is_known_update_kind(kind)
    {
        let content = raw_update
            .and_then(|raw| raw.get("content"))
            .and_then(|raw| raw.get("text"))
            .and_then(serde_json::Value::as_str);
        projection
            .observe_unknown(safe_update_kind(kind).unwrap_or("unrecognized"), content)
            .map_err(|_| ExternalProviderRuntimeError::FrameDecodeFailure)?;
        return Ok((None, false));
    }
    let Ok(typed) = serde_json::from_value::<SessionNotification>(params.clone()) else {
        if crate::provider_update_kind::has_unknown_informational_value(&dispatch) {
            projection
                .observe_unknown(
                    kind.and_then(safe_update_kind).unwrap_or("unrecognized"),
                    None,
                )
                .map_err(|_| ExternalProviderRuntimeError::FrameDecodeFailure)?;
        }
        return Ok((None, false));
    };
    #[cfg(any(test, feature = "test-observation"))]
    observe_tool(&typed.update);
    let prior = catalog.to_session_settings();
    let settings_updated = apply_settings_update(catalog, &typed.update, behavior);
    let settings = catalog.to_session_settings();
    if prior != settings {
        buffer
            .facts()
            .push(StreamFact::Event(SessionEvent::SettingsChanged {
                settings,
            }));
    }
    let text = match &typed.update {
        SessionUpdate::AgentMessageChunk(chunk) => match &chunk.content {
            ContentBlock::Text(content) => Some(content.text.clone()),
            _ => None,
        },
        _ => None,
    };
    if let Err(crate::provider_item_projection::ItemProjectionError::SinkClosed) =
        projection.observe(&typed.update, raw_update)
    {
        return Err(ExternalProviderRuntimeError::SinkClosed);
    }
    Ok((text, settings_updated))
}
