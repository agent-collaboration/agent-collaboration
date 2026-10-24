//! Project cursors retain their writer epoch; fleet positions have no epoch.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{field} is outside its numeric domain")]
pub struct NumericValidationError {
    pub field: &'static str,
}

macro_rules! numeric_value {
    ($name:ident, $minimum:expr, $maximum:expr) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
        #[serde(try_from = "u64", into = "u64")]
        pub struct $name(u64);

        impl TryFrom<u64> for $name {
            type Error = NumericValidationError;
            fn try_from(value: u64) -> Result<Self, Self::Error> {
                if !($minimum..=$maximum).contains(&value) {
                    return Err(NumericValidationError { field: stringify!($name) });
                }
                Ok(Self(value))
            }
        }
        impl From<$name> for u64 {
            fn from(value: $name) -> Self { value.0 }
        }
        impl $name {
            #[must_use]
            pub const fn get(self) -> u64 { self.0 }
        }
        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> { stringify!($name).into() }
            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                schemars::json_schema!({"type":"integer","minimum":$minimum,"maximum":$maximum})
            }
        }
    };
}

numeric_value!(FeedPosition, 0_u64, i64::MAX as u64);
numeric_value!(FleetPosition, 0_u64, i64::MAX as u64);
numeric_value!(HandoffSeq, 0_u64, i64::MAX as u64);
numeric_value!(CancelExpansionSeq, 0_u64, i64::MAX as u64);
numeric_value!(EpochNumber, 1_u64, i64::MAX as u64);
numeric_value!(SubjectRevision, 0_u64, i64::MAX as u64);
numeric_value!(AssignmentRevision, 0_u64, i64::MAX as u64);
numeric_value!(ProtocolVersion, 1_u64, u16::MAX as u64);
numeric_value!(PostCooldownSeconds, 1_u64, 60_u64);

impl FeedPosition {
    pub const ZERO: Self = Self(0);
    pub fn successor(self) -> Result<Self, NumericValidationError> {
        Self::try_from(self.0 + 1)
    }
}
impl FleetPosition {
    pub const ZERO: Self = Self(0);
    pub fn successor(self) -> Result<Self, NumericValidationError> {
        Self::try_from(self.0 + 1)
    }
}
impl HandoffSeq {
    pub const ZERO: Self = Self(0);
    pub fn successor(self) -> Result<Self, NumericValidationError> {
        Self::try_from(self.0 + 1)
    }
}
impl CancelExpansionSeq {
    pub const ZERO: Self = Self(0);
    pub fn successor(self) -> Result<Self, NumericValidationError> {
        Self::try_from(self.0 + 1)
    }
}
impl SubjectRevision {
    pub const INITIAL: Self = Self(0);
    pub fn successor(self) -> Result<Self, NumericValidationError> {
        Self::try_from(self.0 + 1)
    }
}
impl AssignmentRevision {
    pub const INITIAL: Self = Self(0);
    pub fn successor(self) -> Result<Self, NumericValidationError> {
        Self::try_from(self.0 + 1)
    }
}
impl ProtocolVersion {
    pub const CURRENT: Self = Self(1);
}

impl EpochNumber {
    pub const INITIAL: Self = Self(1);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeedCursor {
    pub epoch: EpochNumber,
    pub position: FeedPosition,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "position",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum RecordPlacement {
    Project(ProjectFeedEntry),
    Fleet(FleetFeedEntry),
    Unsequenced,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectFeedEntry {
    pub cursor: FeedCursor,
    pub origin: crate::ActorOrigin,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetFeedEntry {
    pub position: FleetPosition,
    pub origin: crate::ActorOrigin,
}

impl RecordPlacement {
    #[must_use]
    pub fn project_cursor(&self) -> Option<FeedCursor> {
        match self {
            Self::Project(entry) => Some(entry.cursor),
            Self::Fleet(_) | Self::Unsequenced => None,
        }
    }

    #[must_use]
    pub fn fleet_position(&self) -> Option<FleetPosition> {
        match self {
            Self::Fleet(entry) => Some(entry.position),
            Self::Project(_) | Self::Unsequenced => None,
        }
    }

    #[must_use]
    pub fn origin(&self) -> Option<&crate::ActorOrigin> {
        match self {
            Self::Project(entry) => Some(&entry.origin),
            Self::Fleet(entry) => Some(&entry.origin),
            Self::Unsequenced => None,
        }
    }
}
