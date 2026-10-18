use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Board schemas belong to message-board; central transports this envelope
/// without interpreting post contents or the holder's WriteReply.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BoardPayload(Value);
impl BoardPayload {
    #[must_use]
    pub fn new(value: Value) -> Self {
        Self(value)
    }
    #[must_use]
    pub fn as_value(&self) -> &Value {
        &self.0
    }
    #[must_use]
    pub fn into_value(self) -> Value {
        self.0
    }
}
impl From<Value> for BoardPayload {
    fn from(value: Value) -> Self {
        Self::new(value)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ForwardedPostKind {
    ThreadCreate,
    ThreadPost,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ForwardedPost {
    pub kind: ForwardedPostKind,
    pub body: BoardPayload,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ForwardOutcome {
    Answered { reply: BoardPayload },
    SenderHolds,
    NotHeld,
    HolderUnreachable { last_seen: DateTime<Utc> },
    Unanswered,
    Overloaded,
}
pub const FORWARD_IN_FLIGHT_LIMIT: usize = 16;
pub const FORWARD_DEADLINE_SECONDS: i64 = 30;
