//! Canonical custody statuses and recipient reports are separate typed streams.
use crate::{MailDigest, MailItemId, MailSeq};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum CarrierAcceptance {
    Started,
    Steered,
    StartedOrSteered,
    Queued,
    PeerMessageWritten,
    Wake(Box<crate::RemoteWakeResult>),
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum MailStatus {
    Queued {},
    Sent { seq: MailSeq },
    AtMachine {},
    Held {},
    Delivered { acceptance: CarrierAcceptance },
    Uncertain {},
    Expired {},
    Cancelled {},
    Refused { reason: MailRefusal },
}
impl MailStatus {
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Delivered { .. }
                | Self::Uncertain {}
                | Self::Expired {}
                | Self::Cancelled {}
                | Self::Refused { .. }
        )
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum MailRefusal {
    MailboxFull {},
    ItemConflict { existing: MailDigest },
    RecipientUnknown {},
    RecipientRevoked {},
    RecipientUnsupported {},
    DroppedByPolicy {},
    CancelSenderMismatch { target: MailItemId },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TooLateAt {
    Delivered,
    Uncertain,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum CancelResult {
    Pending {},
    Prevented {},
    TooLate { at: TooLateAt },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RecipientOutcome {
    Held {},
    Delivered { acceptance: CarrierAcceptance },
    Uncertain {},
    Expired {},
    Prevented { cancel: MailItemId },
    TooLate { cancel: MailItemId, at: TooLateAt },
    Dropped {},
    Refused { reason: MailRefusal },
}
