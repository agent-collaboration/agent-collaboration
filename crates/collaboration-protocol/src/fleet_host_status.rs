//! Read-only custody observations never change delivery state.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum StuckMailSummary {
    Empty,
    Stuck {
        count: NonZeroU64,
        oldest_age_seconds: u64,
    },
    AgeUnknown {
        recovered_count: NonZeroU64,
        known_stuck: Option<KnownStuckMail>,
    },
    Unavailable {
        message: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnownStuckMail {
    pub count: NonZeroU64,
    pub oldest_age_seconds: u64,
}
