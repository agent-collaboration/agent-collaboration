//! Retained native prompt connection with explicit terminal and callback handoff.
use crate::final_reply_selection::FinalReplySelection;
use crate::{
    AcpSchemaCatalog, AcpSessionBinding, NativeInterruptionState, NativePromptTerminal,
    PromptSettlement, PromptTarget, project_assistant_text, translate_prompt_content,
};
use codex_native_integration::{NativeConnectionError, NativeOperation};
use serde_json::{Value, json};
use session_event_model::managed_interaction_refusal::ManagedInteractionRefusal;

pub enum PromptEvent {
    Update(Value),
    Terminal(Value),
    /// The connection dispatcher must translate and correlate this callback; never auto-approve.
    NativeCallback(Value),
    InteractionRefused {
        id: Value,
        response: Value,
        refusal: ManagedInteractionRefusal,
        interrupt_turn: bool,
    },
    /// Other native items remain available to the tool/plan projection owner.
    NativeNotification(Value),
}
#[derive(Debug, thiserror::Error)]
pub enum PromptExecutionError {
    #[error("invalid ACP prompt")]
    InvalidPrompt,
    #[error("native prompt request failed")]
    Native(#[from] NativeConnectionError),
    #[error("native prompt projection failed")]
    Projection,
    #[error("native session has an active turn")]
    Busy,
    #[error("native thread activity status unavailable")]
    NativeThreadStatusUnavailable,
    #[error("native prompt receipt projection failed at {0}")]
    ReceiptProjection(&'static str),
    #[error("requested effort {requested} but native runtime reported {effective}")]
    EffortMismatch {
        requested: String,
        effective: String,
    },
}
/// Seconds since the thread's own recency, taken from the read Router already made.
///
/// The native Thread schema requires `updatedAt` as unix seconds. A response
/// without it is only idle-free when the thread was never updated, so zero is a
/// fact there; anything else must fail rather than invent a recency.
fn thread_idle_seconds(thread: &Value) -> Result<u64, PromptExecutionError> {
    match thread.get("updatedAt").and_then(Value::as_i64) {
        Some(updated_at) => Ok(u64::try_from(
            chrono::Utc::now().timestamp().saturating_sub(updated_at),
        )
        .unwrap_or(0)),
        None if thread.get("createdAt").and_then(Value::as_i64).is_some() => Ok(0),
        None => Err(PromptExecutionError::ReceiptProjection("recency")),
    }
}

pub struct PendingAcpPrompt {
    detached: bool,
    session: AcpSessionBinding,
    settlement: PromptSettlement,
    final_reply: FinalReplySelection,
    /// Absent on resume, where the thread's persisted effort governs.
    requested_effort: Option<String>,
}
impl PendingAcpPrompt {
    /// Consuming the binding prevents a second concurrent prompt on this mapping.
    pub async fn start(
        session: AcpSessionBinding,
        catalog: &mut AcpSchemaCatalog,
        request_id: Value,
        params: &Value,
    ) -> Result<Self, PromptExecutionError> {
        if !session.schemas.supports_server_messages() {
            return Err(PromptExecutionError::Projection);
        }
        let translated = translate_prompt_content(catalog, params)
            .map_err(|_| PromptExecutionError::InvalidPrompt)?;
        let effort = match params.pointer("/_meta/codexRouter/effort") {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                value
                    .as_str()
                    .filter(|value| {
                        !value.trim().is_empty() && !value.chars().any(char::is_whitespace)
                    })
                    .ok_or(PromptExecutionError::InvalidPrompt)?
                    .to_owned(),
            ),
        };
        if translated.session_id() != session.session_id {
            return Err(PromptExecutionError::InvalidPrompt);
        }
        let mut pending = Self {
            detached: false,
            session,
            settlement: PromptSettlement::new(request_id)
                .map_err(|_| PromptExecutionError::InvalidPrompt)?,
            final_reply: FinalReplySelection::new(),
            requested_effort: effort,
        };
        let thread_state = pending
            .session
            .connection
            .request_validated(
                &pending.session.schemas,
                NativeOperation::ReadThread,
                json!({"threadId":pending.session.session_id,"includeTurns":false}),
            )
            .await?;
        match crate::session_creation::native_thread_activity(&thread_state) {
            crate::session_creation::NativeThreadActivity::Idle => {}
            crate::session_creation::NativeThreadActivity::Active => {
                return Err(PromptExecutionError::Busy);
            }
            crate::session_creation::NativeThreadActivity::Invalid => {
                return Err(PromptExecutionError::NativeThreadStatusUnavailable);
            }
        }
        pending
            .settlement
            .mark_dispatched()
            .map_err(|_| PromptExecutionError::InvalidPrompt)?;
        // Without a requested effort the turn inherits the thread's own.
        let mut turn = json!({"threadId":pending.session.session_id,"input":translated.input()});
        if let (Some(fields), Some(effort)) =
            (turn.as_object_mut(), pending.requested_effort.as_ref())
        {
            fields.insert("effort".into(), json!(effort));
        }
        let result = pending
            .session
            .connection
            .request_validated(&pending.session.schemas, NativeOperation::StartTurn, turn)
            .await?;
        let turn_id = result
            .get("turn")
            .and_then(|turn| turn.get("id"))
            .and_then(Value::as_str)
            .ok_or(PromptExecutionError::Projection)?;
        pending
            .settlement
            .accepted_turn(turn_id.to_owned())
            .map_err(|_| PromptExecutionError::Projection)?;
        pending.session.materialized = true;
        Ok(pending)
    }
    pub async fn next_event(
        &mut self,
        catalog: &mut AcpSchemaCatalog,
    ) -> Result<Option<PromptEvent>, PromptExecutionError> {
        let message = match self.session.connection.next_message().await {
            Ok(message) => message,
            Err(_) => {
                self.detached = true;
                return Ok(self
                    .settlement
                    .observe_terminal(None, NativePromptTerminal::GenerationLost)
                    .map(PromptEvent::Terminal));
            }
        };
        if !self.session.schemas.validates_server_message(&message) {
            self.detached = true;
            return Err(PromptExecutionError::ReceiptProjection(
                "serverMessageSchema",
            ));
        }
        if !self.settlement.is_settled()
            && let Some(turn_id) = self.settlement.turn_id().map(str::to_owned)
        {
            self.final_reply
                .observe_completed_item(&self.session.session_id, &turn_id, &message);
        }
        if let Some(id) = message.get("id")
            && let Some(method) = message.get("method").and_then(Value::as_str)
        {
            let refusal = ManagedInteractionRefusal::ApprovalsUnavailableOnBuild;
            let Some(response) = refusal.codex_response(method) else {
                return Ok(Some(PromptEvent::NativeCallback(message)));
            };
            let params = message
                .get("params")
                .ok_or(PromptExecutionError::Projection)?;
            let turn_id = params.get("turnId").and_then(Value::as_str);
            let matched_turn = turn_id.is_some() && turn_id == self.settlement.turn_id();
            // Codex may emit an MCP elicitation with no active turn at its
            // source. Refuse that request on its own ID without cancelling a
            // turn that happened to start by the time we received it.
            let standalone = method == "mcpServer/elicitation/request" && turn_id.is_none();
            if self.settlement.is_settled()
                || params.get("threadId").and_then(Value::as_str) != Some(self.session.session_id())
                || !(matched_turn || standalone)
            {
                return Ok(None);
            }
            return Ok(Some(PromptEvent::InteractionRefused {
                id: id.clone(),
                response,
                refusal,
                interrupt_turn: matched_turn,
            }));
        }
        if !self.settlement.is_settled()
            && let Some(turn_id) = self.settlement.turn_id()
            && let Some(update) = project_assistant_text(
                catalog,
                PromptTarget {
                    session_id: &self.session.session_id,
                    turn_id,
                },
                &message,
            )
            .map_err(|_| PromptExecutionError::ReceiptProjection("assistantText"))?
        {
            return Ok(Some(PromptEvent::Update(update)));
        }
        if !self.settlement.is_settled()
            && let Some(turn_id) = self.settlement.turn_id()
            && let Some(update) = crate::project_tool_progress(
                catalog,
                PromptTarget {
                    session_id: &self.session.session_id,
                    turn_id,
                },
                &message,
            )
            .map_err(|_| PromptExecutionError::ReceiptProjection("toolProgress"))?
        {
            return Ok(Some(PromptEvent::Update(update)));
        }
        if message.get("method").and_then(Value::as_str) == Some("turn/completed") {
            let params = message
                .get("params")
                .ok_or(PromptExecutionError::ReceiptProjection("terminalParams"))?;
            if params.get("threadId").and_then(Value::as_str) != Some(&self.session.session_id) {
                return Ok(None);
            }
            let turn = params
                .get("turn")
                .ok_or(PromptExecutionError::ReceiptProjection("terminalTurn"))?;
            let status = match turn.get("status").and_then(Value::as_str) {
                Some("completed") => NativePromptTerminal::Completed,
                Some("interrupted") => NativePromptTerminal::Interrupted,
                Some("failed") => NativePromptTerminal::Failed,
                _ => return Err(PromptExecutionError::ReceiptProjection("terminalStatus")),
            };
            let mut terminal = self
                .settlement
                .observe_terminal(turn.get("id").and_then(Value::as_str), status);
            if let Some(response) = terminal
                .as_mut()
                .filter(|response| response.get("result").is_some())
            {
                attach_final_reply(response, self.final_reply.metadata());
                let read = self
                    .session
                    .connection
                    .request_validated(
                        &self.session.schemas,
                        NativeOperation::ReadThread,
                        json!({"threadId":self.session.session_id,"includeTurns":false}),
                    )
                    .await?;
                let thread = read
                    .get("thread")
                    .ok_or(PromptExecutionError::ReceiptProjection("thread"))?;
                let model = thread
                    .get("model")
                    .and_then(Value::as_str)
                    .ok_or(PromptExecutionError::ReceiptProjection("model"))?;
                let effective_effort = thread
                    .get("reasoningEffort")
                    .and_then(Value::as_str)
                    .ok_or(PromptExecutionError::ReceiptProjection("effort"))?;
                // A resume without a requested effort reports what the thread kept.
                // A resume that asks for a different one is allowed, and says so:
                // the provider's prompt cache for this session will not be reused.
                let effort_change = match (&self.requested_effort, &self.session.persisted_effort) {
                    (Some(requested), Some(persisted)) if requested != persisted => {
                        Some(json!({"previous":persisted,"requested":requested}))
                    }
                    _ => None,
                };
                if effort_change.is_none()
                    && let Some(requested) = &self.requested_effort
                    && effective_effort != requested
                {
                    return Err(PromptExecutionError::EffortMismatch {
                        requested: requested.clone(),
                        effective: effective_effort.to_owned(),
                    });
                }
                let idle_seconds = thread_idle_seconds(thread)?;
                let result = response
                    .get_mut("result")
                    .and_then(Value::as_object_mut)
                    .ok_or(PromptExecutionError::ReceiptProjection("result"))?;
                if result.get("_meta").is_some_and(Value::is_null) {
                    result.insert("_meta".into(), json!({}));
                }
                let metadata = result
                    .entry("_meta")
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
                    .ok_or(PromptExecutionError::ReceiptProjection("metadata"))?;
                let mut router_metadata = json!({
                    "effectiveModel": model,
                    "effectiveEffort": effective_effort,
                    "effectiveAccess": self.session.requested_access,
                    "settingsObservation": &self.session.settings_observation,
                    "idleSeconds": idle_seconds
                });
                let fields = router_metadata
                    .as_object_mut()
                    .ok_or(PromptExecutionError::ReceiptProjection("router metadata"))?;
                if !self.session.permission_differences.is_empty() {
                    fields.insert(
                        "permissionDifferences".into(),
                        json!(self.session.permission_differences),
                    );
                }
                if let Some(effort_change) = effort_change {
                    fields.insert("effortChange".into(), effort_change);
                }
                metadata.insert("codexRouter".into(), router_metadata);
            }
            return Ok(terminal.map(PromptEvent::Terminal));
        }
        Ok(Some(PromptEvent::NativeNotification(message)))
    }
    pub async fn cancel(&mut self) -> Option<Value> {
        self.cancel_prompt(false).await
    }

    pub(crate) async fn write_callback_refusal(
        &mut self,
        id: Value,
        response: Value,
    ) -> Result<(), NativeConnectionError> {
        self.session
            .connection
            .submit_callback_response(id, response)
            .await
    }

    pub(crate) async fn cancel_after_refusal(&mut self) -> Option<Value> {
        self.cancel_prompt(true).await
    }

    async fn cancel_prompt(&mut self, reconcile_rejection: bool) -> Option<Value> {
        if self.settlement.is_settled() {
            return None;
        }
        self.settlement.request_cancel();
        let mut response = match tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.cancel_inner(reconcile_rejection),
        )
        .await
        {
            Ok(response) => response,
            Err(_) => {
                self.detached = true;
                self.settlement
                    .settle_cancel(NativeInterruptionState::Unknown)
            }
        };
        if let Some(response) = response.as_mut() {
            attach_final_reply(response, self.final_reply.metadata());
        }
        response
    }
    async fn cancel_inner(&mut self, reconcile_rejection: bool) -> Option<Value> {
        let target = self.settlement.request_cancel().map(str::to_owned);
        let outcome = if let Some(turn_id) = target {
            match self
                .session
                .connection
                .request_validated(
                    &self.session.schemas,
                    NativeOperation::InterruptTurn,
                    json!({"threadId":self.session.session_id,"turnId":turn_id}),
                )
                .await
            {
                Ok(_) => NativeInterruptionState::Confirmed,
                // The refusal may have ended this turn before the interrupt reached Codex.
                Err(NativeConnectionError::Rejected { .. }) if reconcile_rejection => {
                    self.reconcile_refused_turn().await
                }
                Err(NativeConnectionError::Rejected { .. }) => NativeInterruptionState::Rejected,
                Err(_) => NativeInterruptionState::Unknown,
            }
        } else {
            NativeInterruptionState::Unknown
        };
        self.settlement.settle_cancel(outcome)
    }

    async fn reconcile_refused_turn(&mut self) -> NativeInterruptionState {
        let barrier = self.cancellation_barrier();
        match self
            .session
            .connection
            .request_validated(
                &self.session.schemas,
                NativeOperation::ReadThread,
                json!({"threadId":self.session.session_id,"includeTurns":true}),
            )
            .await
        {
            Ok(thread)
                if thread.pointer("/thread/id").and_then(Value::as_str)
                    == Some(self.session.session_id())
                    && barrier.resolved_by_resume(&thread) =>
            {
                NativeInterruptionState::Confirmed
            }
            Ok(_) | Err(NativeConnectionError::Rejected { .. }) => {
                NativeInterruptionState::Rejected
            }
            Err(_) => NativeInterruptionState::Unknown,
        }
    }

    #[must_use]
    pub fn blocks_next_prompt(&self) -> bool {
        self.detached || self.settlement.blocks_next_prompt()
    }
    pub(crate) fn cancellation_barrier(&self) -> crate::CancellationBarrier {
        self.settlement
            .turn_id()
            .map_or(crate::CancellationBarrier::UnknownTurn, |turn| {
                crate::CancellationBarrier::KnownTurn(turn.to_owned())
            })
    }
    pub fn into_session(self) -> Result<AcpSessionBinding, Box<Self>> {
        if self.blocks_next_prompt() {
            Err(Box::new(self))
        } else {
            Ok(self.session)
        }
    }
}

fn attach_final_reply(response: &mut Value, final_reply: Value) {
    let Some(result) = response.get_mut("result").and_then(Value::as_object_mut) else {
        return;
    };
    if result.get("_meta").is_none_or(Value::is_null) {
        result.insert("_meta".to_owned(), json!({}));
    }
    if let Some(metadata) = result.get_mut("_meta").and_then(Value::as_object_mut) {
        metadata.insert("codex-router/finalReply".to_owned(), final_reply);
    }
}

#[cfg(test)]
mod tests {
    use super::attach_final_reply;
    use crate::final_reply_selection::FinalReplySelection;
    use crate::{NativeInterruptionState, NativePromptTerminal, PromptSettlement};
    use serde_json::json;

    fn accepted_prompt_settlement() -> PromptSettlement {
        let mut settlement = PromptSettlement::new(json!("prompt")).expect("prompt ID");
        settlement.mark_dispatched().expect("dispatch");
        settlement
            .accepted_turn("turn-a".to_owned())
            .expect("accepted turn");
        settlement
    }

    #[test]
    fn unavailable_output_does_not_change_completion_and_cancel_keeps_interruption_metadata() {
        for (text, reason) in [
            ("x".repeat(1_048_577), "outputLimitExceeded"),
            ("before\u{0001}after".to_owned(), "outputInvalid"),
        ] {
            let mut selection = FinalReplySelection::new();
            selection.observe_completed_item(
                "thread-a",
                "turn-a",
                &json!({"method":"item/completed","params":{"threadId":"thread-a","turnId":"turn-a","item":{"type":"agentMessage","phase":"final_answer","text":text}}}),
            );
            let mut settlement = accepted_prompt_settlement();
            let mut response = settlement
                .observe_terminal(Some("turn-a"), NativePromptTerminal::Completed)
                .expect("completed result");
            attach_final_reply(&mut response, selection.metadata());

            assert_eq!(response["result"]["stopReason"], "end_turn");
            assert_eq!(
                response["result"]["_meta"]["codex-router/finalReply"],
                json!({"kind":"unavailable","reason":reason})
            );
        }

        let mut selection = FinalReplySelection::new();
        selection.observe_completed_item(
            "thread-a",
            "turn-a",
            &json!({"method":"item/completed","params":{"threadId":"thread-a","turnId":"turn-a","item":{"type":"agentMessage","phase":"final_answer","text":"reply before cancellation"}}}),
        );
        let mut settlement = accepted_prompt_settlement();
        assert_eq!(settlement.request_cancel(), Some("turn-a"));
        let mut response = settlement
            .settle_cancel(NativeInterruptionState::Confirmed)
            .expect("cancelled result");
        attach_final_reply(&mut response, selection.metadata());

        assert_eq!(response["result"]["stopReason"], "cancelled");
        assert_eq!(
            response["result"]["_meta"]["codex-router/nativeInterruption"]["state"],
            "confirmed"
        );
        assert_eq!(
            response["result"]["_meta"]["codex-router/finalReply"],
            json!({"kind":"available","text":"reply before cancellation"})
        );

        let mut no_reply_settlement = accepted_prompt_settlement();
        assert_eq!(no_reply_settlement.request_cancel(), Some("turn-a"));
        let mut no_reply_response = no_reply_settlement
            .settle_cancel(NativeInterruptionState::Confirmed)
            .expect("cancelled before any reply item");
        attach_final_reply(
            &mut no_reply_response,
            FinalReplySelection::new().metadata(),
        );
        assert_eq!(
            no_reply_response["result"]["_meta"]["codex-router/finalReply"],
            json!({"kind":"available","text":null})
        );
    }
}

#[cfg(test)]
#[path = "native_prompt_execution_node_tap_test.rs"]
mod node_tap_tests;
