//! Client-neutral completion and pending evidence for conversation work.
use collaboration_protocol::{
    ConversationOutputUnavailableReason, EffectiveProviderSettings, MessageText, OperationId,
    SessionRef,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
// Keep the public completed-result shape stable; future stop values add one
// string payload but do not justify boxing every existing settlement caller.
pub enum ConversationOperationResult {
    Completed {
        target: SessionRef,
        #[serde(skip_serializing_if = "Option::is_none")]
        operation_id: Option<OperationId>,
        settlement: ConversationSettlement,
    },
    Pending {
        operation_id: OperationId,
        target: SessionRef,
    },
    /// Waiting in the session's line for its running turn to end (D1 R12);
    /// waiting on the operation again gives its completion. The follow-up
    /// says why it waits and how to wait on it, cancel it or interrupt.
    Queued {
        operation_id: OperationId,
        target: SessionRef,
        follow_up: String,
    },
    /// The caller detached from a Codex turn that remains active.
    Running {
        target: SessionRef,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        follow_up: String,
    },
}

impl ConversationOperationResult {
    /// A prompt waiting in its session's line, with the follow-up R12 names.
    #[must_use]
    pub fn queued(operation_id: OperationId, target: SessionRef) -> Self {
        let target_json = serde_json::to_string(&target).unwrap_or_default();
        let operation = operation_id.as_str();
        let follow_up = format!(
            "Queued: the session is in a turn, so this message waits in its line and starts a turn when the running one ends. Wait for it: agent-collaboration conversation operation wait --operation-id {operation}; cancel it: agent-collaboration conversation cancel --target '{target_json}' --target-operation-id {operation}; or interrupt the running turn: agent-collaboration conversation cancel --target '{target_json}' --target-operation-id <the running turn's operation ID>"
        );
        Self::Queued {
            operation_id,
            target,
            follow_up,
        }
    }

    #[must_use]
    pub fn running_codex_turn(target: SessionRef) -> Self {
        let endpoint = String::from(target.endpoint.endpoint_id.clone());
        let session = String::from(target.session_id.clone());
        Self::Running {
            target,
            turn_id: None,
            follow_up: format!(
                "Turn continues. Follow with: agent-collaboration events listen --endpoint {endpoint} --session {session} --attach; or agent-collaboration session inspect --endpoint {endpoint} --session {session} --json"
            ),
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ConversationCreatePromptOutcome {
    CreatePending {
        operation_id: OperationId,
    },
    CreatedWithoutSettings {
        operation_id: OperationId,
        target: SessionRef,
        applied: Vec<collaboration_protocol::AppliedProviderSetting>,
        failed: Vec<collaboration_protocol::FailedProviderSetting>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        not_applied: Vec<collaboration_protocol::NotAppliedProviderSetting>,
    },
    Prompt {
        create_operation_id: OperationId,
        prompt: Box<ConversationOperationResult>,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationSettlement {
    pub target: SessionRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<ConversationStopReason>,
    pub detail: ConversationSettlementDetail,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConversationStopReason {
    Completed,
    TimedOut,
    Cancelled,
    EndTurn,
    MaxTokens,
    MaxTurnRequests,
    Refusal,
    Unknown(String),
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ConversationSettlementDetail {
    CodexPrompt {
        output: ProviderPromptOutput,
        permission_required: bool,
        result: Option<Value>,
    },
    CodexLoad {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        permission_differences: Vec<collaboration_protocol::PermissionDifference>,
    },
    ProviderPrompt {
        output: ProviderPromptOutput,
    },
    ProviderLoad {
        output: ProviderLoadOutput,
    },
    /// Accepted into the running turn (D1 R12); the reply belongs to that
    /// turn. Names the turn's approver when it differs from the one asked.
    ProviderSteered {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_approver: Option<collaboration_protocol::ProviderIdentity>,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ProviderPromptOutput {
    Available {
        text: Option<MessageText>,
    },
    Unavailable {
        reason: ConversationOutputUnavailableReason,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ProviderLoadOutput {
    Available {
        effective_settings: EffectiveProviderSettings,
    },
    Unavailable {
        reason: ConversationOutputUnavailableReason,
    },
}
