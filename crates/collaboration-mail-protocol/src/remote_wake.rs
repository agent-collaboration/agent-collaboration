//! Portable wake requests and owning-machine answers, independent of the Router facade.
use crate::{GenerationGuard, MailDeliveryMode, MailSender, MailValidationError};
use chrono::{DateTime, Utc};
use message_board::{RecordId, SessionRef};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Wake, operation and occurrence identities retain the UUIDv7 wire validator.
macro_rules! wake_identity {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(RecordId);
        impl TryFrom<String> for $name {
            type Error = MailValidationError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                RecordId::try_from(value)
                    .map(Self)
                    .map_err(|_| MailValidationError {
                        field: stringify!($name),
                    })
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0.into()
            }
        }
        impl JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                stringify!($name).into()
            }
            fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
                RecordId::json_schema(generator)
            }
        }
    };
}
wake_identity!(RemoteWakeId);
wake_identity!(RemoteWakeOperationId);
wake_identity!(RemoteWakeChangeId);
wake_identity!(RemoteWakeOccurrenceId);
wake_identity!(RemoteWakeDeliveryId);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RemoteWakeCreator {
    Session(SessionRef),
    Owner {},
    Router {},
    Unrecorded {},
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RemoteWakeContent {
    Agent {
        sender: SessionRef,
        text: RemoteWakeText,
    },
    HumanUser {
        text: RemoteWakeText,
    },
    Router {
        text: RemoteWakeText,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RemoteWakeText(String);
impl TryFrom<String> for RemoteWakeText {
    type Error = MailValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty()
            || value.len() > 1024 * 1024
            || value
                .chars()
                .any(|character| character <= '\u{001f}' && !matches!(character, '\n' | '\t'))
        {
            return Err(MailValidationError { field: "text" });
        }
        Ok(Self(value))
    }
}
impl From<RemoteWakeText> for String {
    fn from(value: RemoteWakeText) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteWakeMessage {
    pub target: SessionRef,
    pub content: RemoteWakeContent,
    pub delivery: MailDeliveryMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_guard: Option<GenerationGuard>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RemoteWakeTiming {
    At {
        at: RemoteWakeTimestamp,
    },
    After {
        seconds: RemoteWakeSeconds,
    },
    Interval {
        seconds: RemoteWakeSeconds,
    },
    Cron {
        expression: String,
        timezone: String,
    },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct RemoteWakeSeconds(u32);
impl TryFrom<u32> for RemoteWakeSeconds {
    type Error = MailValidationError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        (1..=31_536_000)
            .contains(&value)
            .then_some(Self(value))
            .ok_or(MailValidationError { field: "seconds" })
    }
}
impl From<RemoteWakeSeconds> for u32 {
    fn from(value: RemoteWakeSeconds) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RemoteWakeExpiry {
    None,
    At { at: RemoteWakeTimestamp },
    After { seconds: RemoteWakeSeconds },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RemoteWakeRequest {
    Send {
        operation_id: RemoteWakeOperationId,
        replace: bool,
        message: Box<RemoteWakeMessage>,
        timing: RemoteWakeTiming,
        expiry: RemoteWakeExpiry,
    },
    Show {
        wakeup_id: RemoteWakeId,
    },
    Pause {
        operation_id: RemoteWakeOperationId,
        wakeup_id: RemoteWakeId,
    },
    Resume {
        operation_id: RemoteWakeOperationId,
        wakeup_id: RemoteWakeId,
    },
    Cancel {
        operation_id: RemoteWakeOperationId,
        wakeup_id: RemoteWakeId,
    },
    Wait {
        wakeup_id: RemoteWakeId,
        after: Option<String>,
        deadline: RemoteWakeTimestamp,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WakeBody {
    pub request: RemoteWakeRequest,
    pub requested_by: RemoteWakeCreator,
}
impl WakeBody {
    pub fn validate_sender(&self, sender: &MailSender) -> Result<(), MailValidationError> {
        let matches = match (&self.requested_by, sender) {
            (RemoteWakeCreator::Session(creator), MailSender::Session(sender)) => creator == sender,
            (
                RemoteWakeCreator::Owner {}
                | RemoteWakeCreator::Router {}
                | RemoteWakeCreator::Unrecorded {},
                MailSender::HumanUser { .. },
            ) => true,
            _ => false,
        };
        matches.then_some(()).ok_or(MailValidationError {
            field: "requestedBy",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum RemoteWakeState {
    Active,
    Paused,
    Cancelled,
    Expired,
    Finished,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteWakeDefinition {
    pub wakeup_id: RemoteWakeId,
    pub change_id: RemoteWakeChangeId,
    pub created_by: RemoteWakeCreator,
    pub message: RemoteWakeMessage,
    pub timing: RemoteWakeTiming,
    pub anchor_at: RemoteWakeTimestamp,
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(required, schema_with = "Option::<RemoteWakeTimestamp>::json_schema")]
    pub expires_at: Option<RemoteWakeTimestamp>,
    pub created_at: RemoteWakeTimestamp,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteWakeFire {
    pub kind: RemoteWakeFireKind,
    pub wakeup_id: RemoteWakeId,
    pub occurrence_id: RemoteWakeOccurrenceId,
    pub due_at: RemoteWakeTimestamp,
    pub fired_at: RemoteWakeTimestamp,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum RemoteWakeFireKind {
    WakeFired,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteWakeSnapshot {
    pub definition: RemoteWakeDefinition,
    pub state: RemoteWakeState,
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(required, schema_with = "Option::<RemoteWakeTimestamp>::json_schema")]
    pub next_due_at: Option<RemoteWakeTimestamp>,
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(required, schema_with = "Option::<RemoteWakeFire>::json_schema")]
    pub first_fire: Option<RemoteWakeFire>,
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(required, schema_with = "Option::<RemoteWakeDeliveryId>::json_schema")]
    pub pending_delivery_id: Option<RemoteWakeDeliveryId>,
    pub latest_event_cursor: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteWakeMutation {
    pub wakeup: RemoteWakeSnapshot,
    pub discarded_delivery_ids: Vec<RemoteWakeDeliveryId>,
    /// Carrier-specific evidence is opaque here and parsed by the Router facade.
    pub dispatched_deliveries: Vec<serde_json::Value>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RemoteWakeWaitOutcome {
    Fired { fire: RemoteWakeFire },
    Paused,
    Cancelled,
    Expired,
    FinishedWithoutFiring,
    TimedOut,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteWakeWaitResult {
    pub wakeup_id: RemoteWakeId,
    pub outcome: RemoteWakeWaitOutcome,
    pub cursor: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RemoteWakeRefusal {
    InvalidField {
        field: String,
        constraint: String,
    },
    OperationConflict,
    AutomationUnavailable,
    ResourceNotFound,
    InvalidRecord,
    LifecycleConflict,
    Overloaded,
    WakeExists {
        wakeup: RemoteWakeId,
        creator: RemoteWakeCreator,
    },
    TooFarAhead {
        max: u32,
    },
    IntervalTooShort {
        min: u32,
    },
    LifetimeTooLong {
        max: u32,
    },
    ExpiryRequired {},
    RemoteUnavailable,
    DeadlineExpired,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum RemoteWakeMutationState {
    None,
    Committed,
    Unknown,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum RemoteWakeResult {
    Snapshot(Box<RemoteWakeSnapshot>),
    Mutation(Box<RemoteWakeMutation>),
    Wait(RemoteWakeWaitResult),
    Refused {
        reason: RemoteWakeRefusal,
        mutation: RemoteWakeMutationState,
    },
}

/// A complete facade result is at most 1 MiB; the acceptance wrapper gets 1 KiB.
pub const MAX_REMOTE_WAKE_RESULT_BYTES: usize = 1024 * 1024;

impl JsonSchema for RemoteWakeText {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "RemoteWakeText".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"string","minLength":1,"maxLength":1048576,"x-maxUtf8Bytes":1048576,"pattern":"^[^\\u0000-\\u0008\\u000B\\u000C\\u000E-\\u001F]+$"})
    }
}
impl JsonSchema for RemoteWakeSeconds {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "RemoteWakeSeconds".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"integer","minimum":1,"maximum":31536000})
    }
}
/// Preserve the facade's exact UTC spelling, including fractional-second precision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RemoteWakeTimestamp {
    spelling: String,
    instant: DateTime<Utc>,
}
impl TryFrom<String> for RemoteWakeTimestamp {
    type Error = MailValidationError;
    fn try_from(spelling: String) -> Result<Self, Self::Error> {
        if !spelling.ends_with('Z') {
            return Err(MailValidationError { field: "timestamp" });
        }
        let instant = DateTime::parse_from_rfc3339(&spelling)
            .map_err(|_| MailValidationError { field: "timestamp" })?
            .with_timezone(&Utc);
        Ok(Self { spelling, instant })
    }
}
impl From<RemoteWakeTimestamp> for String {
    fn from(value: RemoteWakeTimestamp) -> Self {
        value.spelling
    }
}
impl RemoteWakeTimestamp {
    #[must_use]
    pub const fn as_datetime(&self) -> DateTime<Utc> {
        self.instant
    }
}
impl JsonSchema for RemoteWakeTimestamp {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "RemoteWakeTimestamp".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"string","format":"date-time","pattern":"Z$"})
    }
}
