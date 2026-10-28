//! A record's accepting Router time, without reconstructing missing legacy time.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum RecordTime {
    Router(DateTime<Utc>),
    LegacyUnknown,
}

impl RecordTime {
    #[must_use]
    pub const fn as_router_time(self) -> Option<DateTime<Utc>> {
        match self {
            Self::Router(time) => Some(time),
            Self::LegacyUnknown => None,
        }
    }
}
