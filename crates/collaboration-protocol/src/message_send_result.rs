//! Local carrier observations and remote outbox custody are different send results.
use crate::{MailItemId, PushMessageSendResult, SessionMessageReplyResult, SessionRef};
use schemars::{JsonSchema, Schema};
use serde::{Deserialize, Serialize};

/// A send or reply preserves its existing local receipt, or confirms sender outbox custody.
/// The two local DTOs retain their original field order and complete native evidence.
#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
#[schemars(rename = "MessageSendResult_{TLocal}", transform = disjoint_message_results)]
pub enum MessageSendResult<TLocal = PushMessageSendResult> {
    Local(TLocal),
    Queued { queued: QueuedMessageSend },
}

/// A reply keeps the original reply receipt's JSON when delivered locally.
pub type MessageReplyResult = MessageSendResult<SessionMessageReplyResult>;

#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueuedMessageSend {
    pub item: MailItemId,
    pub target: SessionRef,
    pub target_identity: String,
}

fn disjoint_message_results(schema: &mut Schema) {
    // The strict local receipt cannot contain `queued`, and the strict queued
    // object cannot contain a receipt. Preserve derive's fields and generic names.
    if let Some(variants) = schema.remove("anyOf") {
        schema.insert("oneOf".into(), variants);
    }
}

#[cfg(test)]
#[path = "message_send_result_tests.rs"]
mod tests;
