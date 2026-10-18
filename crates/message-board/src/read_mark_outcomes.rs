//! An identified read marks covered positions; this internal effect has no request key.
use crate::{EpochNumber, FeedCursor, Identity, SchemaRefusal};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ReadActor {
    Anonymous {},
    Identified { reader: Identity },
}
impl ReadActor {
    #[must_use]
    pub fn identity(&self) -> Option<&Identity> {
        match self {
            Self::Anonymous {} => None,
            Self::Identified { reader } => Some(reader),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ReadMarkNotAdvanced {
    NotWriter { current: Option<EpochNumber> },
    LeaseExpired { expired_at: DateTime<Utc> },
    SchemaNotReady { refusal: SchemaRefusal },
    StandDownUnacknowledged {},
    Overloaded {},
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ReadMarkOutcome {
    Advanced { through: FeedCursor },
    Unchanged {},
    NotAdvanced { reason: ReadMarkNotAdvanced },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoardReadReply<TValue> {
    pub value: TValue,
    pub read_mark: Option<ReadMarkOutcome>,
}
