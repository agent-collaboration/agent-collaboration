//! One public message request carries caller intent without selecting a client.
use crate::{CodexGeneration, MessageContent, MessageDelivery, SessionRef};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionMessageSendParams {
    /// A supplied identity enables replay; omission creates a new item at admission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_id: Option<crate::MailItemId>,
    pub target: SessionRef,
    pub message: MessageContent,
    #[serde(default)]
    pub mode: MessageDelivery,
    pub generation_guard: Option<CodexGeneration>,
}
