//! Legacy fields stay exact after the named range, origin-scope and worker projections.
use crate::message_copy::{MessageCopyCounts, MessageUpgradeError};
use chrono::{DateTime, Utc};
use collaboration_protocol::{
    PushDeliveryState, PushKind, PushOrigin, PushRecord, PushRecordDraft, SessionRef,
};
use message_board::{EpochNumber, FeedCursor, FeedPosition, NotifyIntentId, ProjectId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) fn invalid(field: &'static str) -> MessageUpgradeError {
    MessageUpgradeError::InvalidSource { field }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct LegacyPushRow {
    pub push_id: String,
    pub kind: String,
    pub origin_kind: String,
    pub origin_service_id: Option<String>,
    pub origin_endpoint_id: Option<String>,
    pub origin_session_id: Option<String>,
    pub origin_router_ref: Option<String>,
    pub target_service_id: String,
    pub target_endpoint_id: String,
    pub target_session_id: String,
    pub dm_delivery_mode: Option<String>,
    pub dm_generation_guard_json: Option<String>,
    pub reply_to_push_id: Option<String>,
    pub header_facts_json: String,
    pub body: Option<String>,
    pub ranges_json: Option<String>,
    pub delivery_state: String,
    pub last_outcome_json: Option<String>,
    pub created_at: String,
    pub settled_at: Option<String>,
    pub read_at: Option<String>,
}

impl LegacyPushRow {
    pub fn validate(&self) -> Result<(), MessageUpgradeError> {
        let kind: PushKind = serde_json::from_value(serde_json::Value::String(self.kind.clone()))
            .map_err(|_| invalid("push.kind"))?;
        let origin = match self.origin_kind.as_str() {
            "session" if self.origin_router_ref.is_none() => PushOrigin::Session(session(
                self.origin_service_id
                    .as_deref()
                    .ok_or_else(|| invalid("push.origin.session_service"))?,
                self.origin_endpoint_id
                    .as_deref()
                    .ok_or_else(|| invalid("push.origin.session_endpoint"))?,
                self.origin_session_id
                    .as_deref()
                    .ok_or_else(|| invalid("push.origin.session_id"))?,
            )?),
            "owner" | "router"
                if self.origin_service_id.is_none()
                    && self.origin_endpoint_id.is_none()
                    && self.origin_session_id.is_none() =>
            {
                if self.origin_kind == "owner" {
                    if self.origin_router_ref.is_some() {
                        return Err(invalid("push.origin.owner_reference"));
                    }
                    PushOrigin::OwnerUnverified
                } else {
                    let reference = collaboration_protocol::RouterOriginRef::parse_canonical(
                        self.origin_router_ref
                            .as_deref()
                            .ok_or_else(|| invalid("push.origin.missing_router_reference"))?,
                    )
                    .map_err(|_| invalid("push.origin.canonical_reference"))?;
                    if !reference.supports_kind(kind) {
                        return Err(invalid("push.origin.reference_kind"));
                    }
                    PushOrigin::Router(kind)
                }
            }
            _ => return Err(invalid("push.origin.storage_parts")),
        };
        let draft = PushRecordDraft {
            push_id: self
                .push_id
                .clone()
                .try_into()
                .map_err(|_| invalid("push.id"))?,
            kind,
            origin,
            origin_router_ref: self.origin_router_ref.clone(),
            target: session(
                &self.target_service_id,
                &self.target_endpoint_id,
                &self.target_session_id,
            )?,
            mode: self
                .dm_delivery_mode
                .as_ref()
                .map(|mode| serde_json::from_value(serde_json::Value::String(mode.clone())))
                .transpose()
                .map_err(|_| invalid("push.mode"))?,
            guard: self
                .dm_generation_guard_json
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .map_err(|_| invalid("push.guard"))?,
            reply_to_push_id: self
                .reply_to_push_id
                .clone()
                .map(TryInto::try_into)
                .transpose()
                .map_err(|_| invalid("push.reply"))?,
            header_facts: serde_json::from_str(&self.header_facts_json)
                .map_err(|_| invalid("push.header"))?,
            body: self.body.clone(),
            activity: self
                .ranges_json
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .map_err(|_| invalid("push.ranges"))?,
            created_at: timestamp(&self.created_at)?,
        };
        let state = match self.delivery_state.as_str() {
            "pending" => PushDeliveryState::Pending,
            "attempted" => PushDeliveryState::Attempted,
            "held" => PushDeliveryState::Held,
            "delivered" => PushDeliveryState::Delivered,
            "outcome_unknown" => PushDeliveryState::OutcomeUnknown,
            "rejected" => PushDeliveryState::Rejected,
            _ => return Err(invalid("push.state")),
        };
        PushRecord::from_storage_parts(
            draft,
            state,
            self.last_outcome_json
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .map_err(|_| invalid("push.receipt"))?,
            self.settled_at.as_deref().map(timestamp).transpose()?,
            self.read_at.as_deref().map(timestamp).transpose()?,
            None,
        )
        .map_err(|_| invalid("push.snapshot"))?;
        Ok(())
    }
}

fn session(
    service: &str,
    endpoint: &str,
    session: &str,
) -> Result<SessionRef, MessageUpgradeError> {
    Ok(SessionRef {
        endpoint: collaboration_protocol::EndpointRef {
            service_id: service
                .to_owned()
                .try_into()
                .map_err(|_| invalid("push.service"))?,
            endpoint_id: endpoint
                .to_owned()
                .try_into()
                .map_err(|_| invalid("push.endpoint"))?,
        },
        session_id: session
            .to_owned()
            .try_into()
            .map_err(|_| invalid("push.session"))?,
    })
}
fn timestamp(value: &str) -> Result<DateTime<Utc>, MessageUpgradeError> {
    if !value.ends_with('Z') {
        return Err(invalid("push.timestamp"));
    }
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| invalid("push.timestamp"))
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SubscriptionRow {
    pub project_id: String,
    pub membership_id: String,
    pub activated_epoch: i64,
    pub activated_position: i64,
    pub payload: String,
}
impl SubscriptionRow {
    pub fn project_payload(&mut self) -> Result<(), MessageUpgradeError> {
        // Only these three fields are retired by 910. Other fields must survive
        // this projection and pass the new owning payload constructor.
        let mut payload: serde_json::Value =
            serde_json::from_str(&self.payload).map_err(|_| invalid("subscription.payload"))?;
        if let Some(roots) = payload
            .get_mut("roots")
            .and_then(serde_json::Value::as_array_mut)
        {
            for root in roots {
                if let Some(window) = root
                    .get_mut("window")
                    .and_then(serde_json::Value::as_object_mut)
                {
                    for field in ["heldSince", "nextRetryAt", "retryAttempts"] {
                        window.remove(field);
                    }
                }
            }
        }
        self.payload = serde_json::to_string(&payload)?;
        self.validate()
    }
    pub fn validate(&self) -> Result<(), MessageUpgradeError> {
        let progress: interaction_storage::SubscriptionDeliveryProgress =
            serde_json::from_str(&self.payload).map_err(|_| invalid("subscription.payload"))?;
        if progress.project().as_str() != self.project_id
            || progress.membership().as_str() != self.membership_id
            || progress.activated_at() != cursor(self.activated_epoch, self.activated_position)?
        {
            return Err(invalid("subscription.naturalKey"));
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct IntentRow {
    pub project_id: String,
    pub intent_id: String,
    pub state_epoch: i64,
    pub state_position: i64,
}
impl IntentRow {
    pub fn validate(&self) -> Result<(), MessageUpgradeError> {
        ProjectId::try_from(self.project_id.clone()).map_err(|_| invalid("intent.project"))?;
        NotifyIntentId::try_from(self.intent_id.clone()).map_err(|_| invalid("intent.id"))?;
        cursor(self.state_epoch, self.state_position)?;
        Ok(())
    }
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct IntentCursorRow {
    pub project_id: String,
    pub epoch: i64,
    pub position: i64,
}
impl IntentCursorRow {
    pub fn validate(&self) -> Result<(), MessageUpgradeError> {
        ProjectId::try_from(self.project_id.clone()).map_err(|_| invalid("cursor.project"))?;
        cursor(self.epoch, self.position)?;
        Ok(())
    }
}
fn cursor(epoch: i64, position: i64) -> Result<FeedCursor, MessageUpgradeError> {
    Ok(FeedCursor {
        epoch: EpochNumber::try_from(u64::try_from(epoch).map_err(|_| invalid("cursor.epoch"))?)
            .map_err(|_| invalid("cursor.epoch"))?,
        position: FeedPosition::try_from(
            u64::try_from(position).map_err(|_| invalid("cursor.position"))?,
        )
        .map_err(|_| invalid("cursor.position"))?,
    })
}

#[derive(Clone, Default)]
pub(crate) struct MessageRows {
    pub pushes: Vec<LegacyPushRow>,
    pub subscriptions: Vec<SubscriptionRow>,
    pub intents: Vec<IntentRow>,
    pub cursors: Vec<IntentCursorRow>,
}
impl MessageRows {
    pub fn counts(&self) -> MessageCopyCounts {
        MessageCopyCounts {
            pushes: self.pushes.len(),
            subscription_progress: self.subscriptions.len(),
            intent_progress: self.intents.len(),
            intent_cursors: self.cursors.len(),
        }
    }
    pub fn compare(&self, other: &Self) -> Result<(), MessageUpgradeError> {
        // Source ranges and SubscriptionExpiry origin scopes already contain
        // translate(source), as named by MESSAGE_COPY_DIGEST_BASIS. Other push
        // columns remain byte-exact.
        compare("router_pushes", &self.pushes, &other.pushes)?;
        compare(
            "subscription_delivery_progress",
            &self.subscriptions,
            &other.subscriptions,
        )?;
        compare("board_intent_progress", &self.intents, &other.intents)?;
        compare("board_intent_scan_cursors", &self.cursors, &other.cursors)
    }
}
fn compare<T: Serialize>(
    table: &'static str,
    expected: &[T],
    stored: &[T],
) -> Result<(), MessageUpgradeError> {
    if expected.len() != stored.len() {
        return Err(MessageUpgradeError::CopyMismatch { table });
    }
    for (expected, stored) in expected.iter().zip(stored) {
        let expected = Sha256::digest(serde_json::to_vec(expected)?);
        let stored = Sha256::digest(serde_json::to_vec(stored)?);
        if expected != stored {
            return Err(MessageUpgradeError::CopyMismatch { table });
        }
    }
    Ok(())
}
