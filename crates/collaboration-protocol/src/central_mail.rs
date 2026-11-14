//! Central's typed mail and timer contract; delivery belongs to a later boundary.
use std::num::NonZeroU32;

use agent_automation::TimingRule;
use serde::{Deserialize, Serialize};
use workspace_ids::{
    ActorIdentityId, AutomationId, ChangeId, MessageId, RequestId, ThreadId, WakeId, WorkspaceId,
};
use workspace_model::{
    ChangeEvent, ChangePosition, ItemRef, PostText, RefusalReason, SessionAddress, Timestamp,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CentralMailRequest {
    pub request_id: RequestId,
    pub operation: CentralMailOperation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CentralMailOperation {
    SendMessage {
        to: SessionAddress,
        body: PostText,
    },
    SetWake {
        target: SessionAddress,
        due_at: Timestamp,
        text: PostText,
    },
    CancelWake {
        wake_id: WakeId,
    },
    SetAutomation {
        target: SessionAddress,
        timing: TimingRule,
        text: PostText,
    },
    DisableAutomation {
        automation_id: AutomationId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CentralMailAnswer {
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CentralMailRefusal {
    TimeOutOfRange { field: String },
    Workspace(RefusalReason),
    UnknownSession { target: SessionAddress },
    NoOccurrence,
    InvalidInput { field: String },
    WakeNotFound { wake_id: WakeId },
    WakeAlreadyFired { wake_id: WakeId },
    AutomationNotFound { automation_id: AutomationId },
    AutomationAlreadyDone { automation_id: AutomationId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MessageBody {
    Direct {
        body: PostText,
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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NoticeItem {
    pub change_id: ChangeId,
    pub position: ChangePosition,
    pub item: ItemRef,
    pub actor: ActorIdentityId,
    pub event: ChangeEvent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageKind {
    Direct,
    Notice,
    Wake,
    Automation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum MessageState {
    Queued,
    Delivered,
    Cancelled,
    Failed { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageRecord {
    pub message_id: MessageId,
    pub sender: MessageSender,
    pub target: SessionAddress,
    pub body: MessageBody,
    pub state: MessageState,
    pub source: Option<NoticeSource>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NoticeSource {
    pub change_id: ChangeId,
    pub recipient_actor: ActorIdentityId,
    pub workspace_id: WorkspaceId,
    pub thread_id: Option<ThreadId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WakeState {
    Active,
    Done,
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AutomationState {
    Active,
    Done,
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MailPolicy {
    notification_window_seconds: NonZeroU32,
}

impl MailPolicy {
    #[must_use]
    pub const fn new(notification_window_seconds: NonZeroU32) -> Self {
        Self {
            notification_window_seconds,
        }
    }

    #[must_use]
    pub const fn window_seconds(self) -> u32 {
        self.notification_window_seconds.get()
    }
}

impl Default for MailPolicy {
    fn default() -> Self {
        Self {
            notification_window_seconds: NonZeroU32::new(900).unwrap_or(NonZeroU32::MIN),
        }
    }
}

#[cfg(test)]
#[path = "central_mail_test.rs"]
mod tests;
