//! Sender custody is typed separately from native pushes.
use crate::{
    MailDigest, MailItemId, PushOrigin, PushRecordShowParams, PushRecordShowResult, SessionRef,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum MessageShowResult {
    Local(Box<PushRecordShowResult>),
    Remote { mail: Box<RemoteMessageStatus> },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteMessageStatus {
    pub item: MailItemId,
    pub context: MailReadContext,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
    pub status: RemoteDeliveryStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum MessageHistoryEntry {
    Local(crate::PushRecordNotice),
    Remote { mail: RemoteMessageStatus },
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageHistoryResult {
    pub records: Vec<MessageHistoryEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MailReadContext {
    Direct {
        origin: PushOrigin,
        recipient: SessionRef,
    },
    Project {
        project: message_board::ProjectId,
        handoff: message_board::MailItemRef,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum HumanMessageReader {
    HumanUser,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum MessageReadCaller {
    Session(SessionRef),
    Human(HumanMessageReader),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageShowRequest {
    pub caller: MessageReadCaller,
    pub reference: String,
}
impl From<PushRecordShowParams> for MessageShowRequest {
    fn from(request: PushRecordShowParams) -> Self {
        Self {
            caller: MessageReadCaller::Session(request.caller),
            reference: request.reference,
        }
    }
}

// The SDK owns custody. These public DTOs preserve its complete closed status
// surface because its Rust enums do not implement our API's JSON Schema trait.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RemoteDeliveryStatus {
    Queued,
    Sent {
        seq: u64,
    },
    AtMachine,
    Held,
    Delivered {
        acceptance: collaboration_mail_protocol::CarrierAcceptance,
    },
    Uncertain,
    Expired,
    Cancelled,
    Refused {
        reason: RemoteDeliveryRefusal,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RemoteDeliveryRefusal {
    RecipientRefused {
        code: String,
    },
    ProcessingFailed {
        failure: RemoteProcessingFailure,
    },
    TooLarge {
        field: RemoteSizeField,
    },
    MailboxFull,
    ItemConflict {
        existing: MailDigest,
    },
    CancelAlreadyRequested {
        cancel: MailItemId,
        digest: MailDigest,
    },
    RecipientUnknown,
    RecipientRevoked,
    DroppedByPolicy,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum RemoteProcessingFailure {
    CustodyMissing,
    CustodyUnresolvable,
    CustodyCorrupt,
    DigestMismatch,
    VersionMismatch,
    Nondeterministic,
    InvalidPayload,
    ExecutionFailed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum RemoteSizeField {
    Sender,
    Address,
    Acceptance,
    StatusRoute,
}
