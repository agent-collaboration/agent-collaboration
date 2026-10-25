//! Aggregate cursors retain each project's position; they promise no global snapshot.
use crate::{FeedCursor, FeedPosition, ProjectId};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AggregateCursor {
    pub per_project: BTreeMap<ProjectId, FeedCursor>,
    pub sweep: Option<MergeKey>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MergeKey {
    pub time: MergeTime,
    pub project_id: ProjectId,
    pub position: FeedPosition,
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, JsonSchema,
)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum MergeTime {
    Unknown,
    Known(DateTime<Utc>),
}

impl From<crate::RecordTime> for MergeTime {
    fn from(time: crate::RecordTime) -> Self {
        match time {
            crate::RecordTime::LegacyUnknown => Self::Unknown,
            crate::RecordTime::Router(time) => Self::Known(time),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ReadPosition {
    Project { cursor: FeedCursor },
    Aggregate { cursor: AggregateCursor },
}

impl ReadPosition {
    #[must_use]
    pub fn precedes_or_equals(&self, upper: &Self) -> bool {
        match (self, upper) {
            (Self::Project { cursor: lower }, Self::Project { cursor: upper }) => {
                lower.position <= upper.position
            }
            (Self::Aggregate { cursor: lower }, Self::Aggregate { cursor: upper }) => {
                lower.per_project.iter().all(|(project, lower)| {
                    upper
                        .per_project
                        .get(project)
                        .is_some_and(|upper| lower.position <= upper.position)
                })
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AggregatePage<TRecord> {
    pub records: Vec<TRecord>,
    pub next: AggregateCursor,
    pub high_water: BTreeMap<ProjectId, FeedCursor>,
    pub unavailable: Vec<ProjectId>,
}
