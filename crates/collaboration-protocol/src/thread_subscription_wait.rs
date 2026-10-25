//! Typed wire contract for bounded thread subscription waits.
use crate::{MessageText, PushId};
use chrono::{DateTime, Utc};
use message_board::public_replies::WriteReply;
use message_board::{
    ActingForIdentity, BoardError, Identity, PendingRootNotice, ProjectId, ProtocolVersion,
    RequestId, SubscriptionTarget, ThreadRef,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_SUBSCRIPTION_WAIT_SECONDS: u64 = 1500;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ThreadSubscriptionWaitFilter {
    All {},
    Roots { threads: Vec<ThreadRef> },
    Project { project_id: ProjectId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    rename_all = "camelCase",
    deny_unknown_fields,
    try_from = "RawThreadSubscriptionWaitRequest"
)]
pub struct ThreadSubscriptionWaitRequest {
    pub actor: Identity,
    pub acting_for: Option<ActingForIdentity>,
    pub protocol_version: ProtocolVersion,
    pub renewals: Vec<SubscriptionRenewalEntry>,
    pub filter: ThreadSubscriptionWaitFilter,
    #[schemars(range(max = 1500))]
    pub max_wait_seconds: u64,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawThreadSubscriptionWaitRequest {
    actor: Identity,
    acting_for: Option<ActingForIdentity>,
    protocol_version: ProtocolVersion,
    renewals: Vec<SubscriptionRenewalEntry>,
    filter: ThreadSubscriptionWaitFilter,
    #[schemars(range(max = 1500))]
    max_wait_seconds: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("maxWaitSeconds must be at most 1500 seconds")]
pub enum InvalidSubscriptionWait {
    Deadline,
    #[error("renewals must contain at most 100 distinct scopes")]
    Renewals,
}

impl TryFrom<RawThreadSubscriptionWaitRequest> for ThreadSubscriptionWaitRequest {
    type Error = InvalidSubscriptionWait;

    fn try_from(raw: RawThreadSubscriptionWaitRequest) -> Result<Self, Self::Error> {
        let request = Self {
            actor: raw.actor,
            acting_for: raw.acting_for,
            protocol_version: raw.protocol_version,
            renewals: raw.renewals,
            filter: raw.filter,
            max_wait_seconds: raw.max_wait_seconds,
        };
        request.validate()?;
        Ok(request)
    }
}

impl ThreadSubscriptionWaitRequest {
    pub fn validate(&self) -> Result<(), InvalidSubscriptionWait> {
        if self.max_wait_seconds > MAX_SUBSCRIPTION_WAIT_SECONDS {
            return Err(InvalidSubscriptionWait::Deadline);
        }
        if self.renewals.len() > 100
            || self.renewals.iter().enumerate().any(|(index, entry)| {
                self.renewals
                    .iter()
                    .take(index)
                    .any(|previous| previous.scope.kind_and_id() == entry.scope.kind_and_id())
            })
        {
            return Err(InvalidSubscriptionWait::Renewals);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SubscriptionWaitBatch {
    Notice {
        push_id: PushId,
        line: MessageText,
        held: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        held_since: Option<DateTime<Utc>>,
        draining: bool,
        roots: Vec<PendingRootNotice>,
    },
    Ranges {
        held: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        held_since: Option<DateTime<Utc>>,
        draining: bool,
        roots: Vec<PendingRootNotice>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThreadSubscriptionWaitResult {
    pub renewals: Vec<SubscriptionRenewalOutcome>,
    pub outcome: ThreadSubscriptionWaitOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionRenewalEntry {
    pub request_id: RequestId,
    pub scope: SubscriptionTarget,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionRenewalOutcome {
    pub request_id: RequestId,
    pub scope: SubscriptionTarget,
    pub reply: WriteReply,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ThreadSubscriptionWaitOutcome {
    Batch { batch: SubscriptionWaitBatch },
    TimedOut {},
    Refused { reason: BoardError },
}
