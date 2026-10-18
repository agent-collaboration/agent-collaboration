//! Typed per-root range locators for subscription notices and their settlement.
use super::{
    BatchId, InvalidSubscriptionField, SubscriptionGeneration, SubscriptionScope,
    SubscriptionWindowId, invalid_subscription_field,
};
use crate::{FeedCursor, ProjectId, ThreadId};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Activity locator and summary for the pending messages in one subscribed root.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PendingRootNotice {
    /// The Thread whose pending messages this notice locates.
    pub thread_id: ThreadId,
    /// The Project containing the Thread.
    pub project_id: ProjectId,
    /// The first pending message sequence, greater than the Reader's Delivered cursor.
    pub from: FeedCursor,
    /// The latest pending message sequence included by this notice.
    pub through: FeedCursor,
    /// Number of pending messages between the two sequence bounds.
    pub post_count: u64,
}

impl PendingRootNotice {
    pub fn new(
        thread_id: ThreadId,
        project_id: ProjectId,
        from: FeedCursor,
        through: FeedCursor,
        post_count: u64,
    ) -> Result<Self, InvalidSubscriptionField> {
        if from.position > through.position {
            return Err(invalid_subscription_field(
                "from",
                "must not exceed through",
            ));
        }
        if post_count == 0 {
            return Err(invalid_subscription_field(
                "postCount",
                "must be greater than zero",
            ));
        }
        Ok(Self {
            thread_id,
            project_id,
            from,
            through,
            post_count,
        })
    }
}

/// Selection state shared by the per-Reader delivery owner and its storage settlement.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionBatch {
    /// Identifier for this selected notice operation.
    pub batch_id: BatchId,
    /// Whether the notice has been held while its target was not running.
    pub held: bool,
    /// First time the notice entered the held state.
    pub held_since: Option<DateTime<Utc>>,
    /// Whether any selected root is draining after resolution.
    pub draining: bool,
    /// Per-root range locators; no message bodies are included.
    pub roots: Vec<PendingRootNotice>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionBatchRootSettlement {
    pub thread_id: ThreadId,
    pub window_id: SubscriptionWindowId,
    pub delivered_through: FeedCursor,
    pub subscription_scope: SubscriptionScope,
    pub subscription_generation: SubscriptionGeneration,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionBatchSettlement {
    pub roots: Vec<SubscriptionBatchRootSettlement>,
}
