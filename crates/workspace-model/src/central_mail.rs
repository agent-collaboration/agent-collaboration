//! Central's typed mail and timer contract; delivery belongs to a later boundary.
use crate::{ChangeEvent, ChangePosition, ItemRef, PostText, RefusalReason, Timestamp};
use agent_automation::TimingRule;
use serde::{Deserialize, Serialize};
use workspace_ids::SessionRef;
use workspace_ids::{
    ActorIdentityId, AutomationId, ChangeId, MessageId, RequestId, ThreadId, WakeId, WorkspaceId,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CentralMailRequest {
    pub request_id: RequestId,
    pub operation: CentralMailOperation,
}

/// The agent's send request. The Router stamps the wire operation from its outbox.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SendMessageRequest {
    pub to: SessionRef,
    pub body: PostText,
    pub delivery: MessageDelivery,
    #[serde(deserialize_with = "present_option")]
    #[schemars(required, schema_with = "Option::<MessageId>::json_schema")]
    pub reply_to: Option<MessageId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CentralMailOperation {
    SetNoticeCadence {
        cadence: crate::NoticeCadence,
    },
    SendMessage {
        to: SessionRef,
        body: PostText,
        /// Required: a missing field is refused, never defaulted.
        delivery: MessageDelivery,
        /// The message this one answers, carried to the recipient as-is;
        /// required (null when it answers none).
        #[serde(deserialize_with = "present_option")]
        reply_to: Option<MessageId>,
        /// When the agent sent it, on the sender Mac's clock (the outbox row's
        /// creation stamp). Central counts the one 15-minute delivery window
        /// from it (owner, 2026-11-17), clamped to its own acceptance time.
        sent_at: Timestamp,
    },
    SetWake {
        target: SessionRef,
        /// Future UTC five-minute mark, with zero fractional seconds, within seven days.
        due_at: Timestamp,
        text: PostText,
    },
    CancelWake {
        wake_id: WakeId,
    },
    SetAutomation {
        target: SessionRef,
        /// At uses UTC five-minute marks; durations are positive multiples of 300
        /// seconds; cron selects only five-minute values and UTC-aligned occurrences.
        timing: TimingRule,
        text: PostText,
    },
    DisableAutomation {
        automation_id: AutomationId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CentralMailAnswer {
    NoticeCadenceSet {
        cadence: crate::NoticeCadence,
    },
    Queued {
        message_id: MessageId,
    },
    WakeSet {
        wake_id: WakeId,
        due_at: Timestamp,
    },
    WakeCancelled {
        wake_id: WakeId,
    },
    AutomationSet {
        automation_id: AutomationId,
        next_due_at: Option<Timestamp>,
        state: AutomationState,
    },
    AutomationDisabled {
        automation_id: AutomationId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CentralMailResult {
    Done(CentralMailAnswer),
    Refused(CentralMailRefusal),
    Conflict,
    OutcomeUnknown,
    CentralUnavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CentralMailRefusal {
    NotSession,
    TimeOutOfRange {
        field: String,
    },
    TimingViolation {
        field: String,
        requirement: TimingRequirement,
    },
    Workspace(RefusalReason),
    UnknownSession {
        target: SessionRef,
    },
    NoOccurrence,
    InvalidInput {
        field: String,
    },
    WakeNotFound {
        wake_id: WakeId,
    },
    WakeAlreadyFired {
        wake_id: WakeId,
    },
    AutomationNotFound {
        automation_id: AutomationId,
    },
    AutomationAlreadyDone {
        automation_id: AutomationId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TimingRequirement {
    FiveMinuteGrid,
    WithinSevenDays,
    FutureTime,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MessageBody {
    Direct {
        body: PostText,
        /// Stored inside the message body JSON; required on decode.
        delivery: MessageDelivery,
        /// The message this one answers; required on decode (null for none).
        #[serde(deserialize_with = "present_option")]
        reply_to: Option<MessageId>,
    },
    Notice {
        items: Vec<NoticeItem>,
    },
    Wake {
        wake_id: WakeId,
        text: PostText,
    },
    Automation {
        automation_id: AutomationId,
        text: PostText,
    },
    /// Central's update to the sender after a refusal, cancellation or late
    /// acceptance. It has a 24-hour delivery window and produces no outcome.
    DeliveryOutcome {
        message_id: MessageId,
        /// The sender Router's outbox request that created the message.
        request_id: RequestId,
        outcome: MessageOutcome,
    },
}

/// How a direct message ended, when the sender is told. A normal delivery
/// sends nothing: the sender's receipt already covers it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MessageOutcome {
    /// Accepted by the recipient after central had cancelled it (a late Ack).
    DeliveredAfterAll,
    /// The recipient's Router refused it for good.
    Refused { reason: MessageRefusal },
    /// Not delivered within 15 minutes of when the agent sent it (owner,
    /// record:218: "15 minutes you can't send in central or machine then tell
    /// the sender and cancel send").
    Cancelled { reason: MessageCancellation },
}

/// Why the recipient refused a message.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum MessageRefusal {
    /// The receiving Router has no harness that owns this session.
    UnknownSession,
    /// More than one local harness claims the same provider session id.
    AmbiguousSession,
}

/// The recipient Router's answer to a delivered message: into its inbox, or
/// refused for good. A valid message always goes into the inbox (owner,
/// record:297), so there is no "send it again later".
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DeliveryAnswer {
    Accepted,
    Refused { reason: MessageRefusal },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum MessageCancellation {
    NotDeliveredWithin15Minutes,
}

/// How the recipient Router hands a direct message to its session: its own
/// choice, queued for the next turn, or steered into the running turn.
/// The sender's view of a direct message's delivery.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum MessageStatus {
    Queued,
    Delivered,
    Failed { reason: MessageRefusal },
    Cancelled { reason: MessageCancellation },
}

#[derive(
    Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum MessageDelivery {
    #[default]
    Auto,
    Queue,
    Steer,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NoticeItem {
    pub workspace_id: WorkspaceId,
    pub thread_id: Option<ThreadId>,
    pub change_id: ChangeId,
    pub position: ChangePosition,
    pub item: ItemRef,
    pub actor: ActorIdentityId,
    pub event: ChangeEvent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum MessageKind {
    Direct,
    Notice,
    Wake,
    Automation,
    DeliveryOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum MessageState {
    Queued,
    Delivered,
    Cancelled,
    Failed { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum MessageSender {
    Actor(ActorIdentityId),
    Central,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageRecord {
    pub message_id: MessageId,
    pub sender: MessageSender,
    pub target: SessionRef,
    pub body: MessageBody,
    pub state: MessageState,
    pub source: Option<NoticeSource>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// Non-destructive view of one session's queued messages in stable UUIDv7 order.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionQueue {
    pub session: SessionRef,
    pub messages: Vec<MessageRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NoticeSource {
    pub change_id: ChangeId,
    pub recipient_actor: ActorIdentityId,
    pub workspace_id: WorkspaceId,
    pub thread_id: Option<ThreadId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum WakeState {
    Active,
    Done,
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AutomationState {
    Active,
    Done,
    Disabled,
}

#[cfg(test)]
#[path = "central_mail_test.rs"]
mod tests;

/// An Option field that must be present (null allowed). Serde's derive would
/// otherwise decode a missing Option as None; a `deserialize_with` field is
/// required, so a missing `replyTo` is refused, never coerced (AGENTS.md).
fn present_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
