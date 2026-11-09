//! The S1 event record and the shell's other conformance lines, as they go on the wire.
//!
//! Times are integer Unix milliseconds. A field that doesn't apply is JSON
//! `null`, never absent. Fields slice 3 and 4 fill (attempts, subjects, notice
//! ranges, per-source batch counts) are null or empty until then.

use crate::ObjectAddress;
use durable_steps::Uuid;
use fleet_protocol::ConnectionId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventRecord {
    pub object: ObjectAddress,
    /// The kernel instance that handled the event.
    pub construction: Uuid,
    /// Starts at 0 per construction and is assigned when the event completes,
    /// so entry order is `entered_at`, not `sequence`.
    pub sequence: u64,
    pub event_id: Uuid,
    pub event: EventKindRecord,
    pub entered_at: i64,
    pub exited_at: i64,
    pub units: Vec<UnitRecord>,
    pub sends: Vec<SendRecord>,
    pub notices: Vec<NoticeRecord>,
    pub calls: Vec<CallRecord>,
    pub alarm: AlarmRecord,
    pub batch: Option<BatchRecord>,
    pub reply: ReplyRecord,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum EventKindRecord {
    SocketOpened {
        connection: ConnectionId,
    },
    Frame {
        connection: ConnectionId,
        #[serde(with = "base64_bytes")]
        frame: Vec<u8>,
    },
    SocketClosed {
        connection: ConnectionId,
        cause: CloseCause,
    },
    Call {
        call_kind: Option<String>,
    },
    /// `redelivery` is null where the platform doesn't report it (Cloudflare).
    Alarm {
        redelivery: Option<u32>,
        delivery: AlarmDelivery,
    },
}

/// Who delivered an Alarm event (seam contract v3.8).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AlarmDelivery {
    /// The platform's own alarm entry, woken by time. Tokio's alarm loop counts as platform.
    #[default]
    Platform,
    /// A conformance `settleClock` on workerd, through the same entry the platform calls.
    Injected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CloseCause {
    Callback,
    InventoryReconcile,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UnitRecord {
    pub unit_kind: String,
    pub subject: Option<Uuid>,
    pub outcome: UnitOutcomeRecord,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum UnitOutcomeRecord {
    Done,
    Deferred {
        retry_at: i64,
    },
    Failed {
        error_class: String,
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SendRecord {
    pub attempt: Option<Uuid>,
    pub attempt_kind: Option<String>,
    pub subject: Option<Uuid>,
    pub send_id: Option<Uuid>,
    pub connection: ConnectionId,
    pub session: Option<Uuid>,
    /// The frame's top-level `kind`, when the frame is a JSON object that has one.
    pub request_kind: Option<String>,
    pub sent_version: Option<u64>,
    pub batch: Option<SendBatchRecord>,
    #[serde(with = "base64_bytes")]
    pub frame: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SendBatchRecord {
    pub batch_send_id: Uuid,
    pub index: u32,
    pub items: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NoticeRecord {
    pub machine: Uuid,
    pub from: u64,
    pub through: u64,
    pub cause: NoticeCause,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NoticeCause {
    Live,
    Welcome,
    Resend,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CallRecord {
    pub to: ObjectAddress,
    /// The call envelope's `kind`, when the body is a JSON object that has one.
    pub call_kind: Option<String>,
    /// The pipeline step (or, from slice 3, the unit) that made the call.
    pub unit_kind: Option<String>,
    pub subject: Option<Uuid>,
    pub outcome: CallOutcomeRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CallOutcomeRecord {
    Answered,
    /// The event stopped waiting before an answer arrived.
    DeadlineExceeded,
    Failed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AlarmRecord {
    pub before: Option<i64>,
    pub after: Option<i64>,
    pub writes: Vec<AlarmWriteRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AlarmWriteRecord {
    pub op: AlarmWriteOp,
    /// For `exact`, null deletes the alarm.
    pub at: Option<i64>,
    pub unit_kind: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AlarmWriteOp {
    /// A unit moved the alarm earlier (never later).
    Lower,
    /// The recompute or the repair set the alarm to exactly this time.
    Exact,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BatchRecord {
    /// Whether this event's claim attempt won the batch.
    pub claimed: bool,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub sources: Vec<BatchSourceRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BatchSourceRecord {
    pub source: String,
    pub done: u32,
    pub failed: u32,
    pub remaining: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ReplyRecord {
    None,
    SocketFailed,
    CallAnswer,
    CallFailure,
    AlarmDone,
    AlarmRetry,
}

/// Emitted by the shell each time it builds a kernel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConstructionLine {
    pub object: ObjectAddress,
    pub construction: Uuid,
    pub cause: ConstructionCause,
    /// Minted once per shell process (Tokio) or Rust `DurableObject::new` (Cloudflare).
    pub isolate: Uuid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConstructionCause {
    Start,
    Rebuild,
    Isolate,
}

/// Emitted when `alarmDelivery` is off and the shell drops an alarm event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AlarmDroppedLine {
    pub object: ObjectAddress,
    pub at: i64,
}

/// One stderr line per record, so a quiet interval is observable without
/// asking the object anything.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConformanceLine<'line> {
    CentralEvent(&'line EventRecord),
    CentralConstruction(&'line ConstructionLine),
    CentralAlarmDropped(&'line AlarmDroppedLine),
}

impl ConformanceLine<'_> {
    /// The one JSON line, for a shell whose platform has its own console.
    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|error| {
            format!("{{\"centralLineEncodeFailed\":{:?}}}", error.to_string())
        })
    }

    /// Writes the line to stderr; native shells only (stderr goes nowhere on wasm).
    pub fn emit(&self) {
        eprintln!("{}", self.encode());
    }
}

mod base64_bytes {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        base64::engine::general_purpose::STANDARD
            .decode(text)
            .map_err(serde::de::Error::custom)
    }
}
