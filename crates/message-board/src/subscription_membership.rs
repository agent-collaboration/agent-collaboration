//! Replicated membership facts contain no Router-local delivery progress.
use crate::{
    EndReason, FeedCursor, Identity, InvalidSubscriptionField, RecordEnvelope, RecordPosition,
    SubscriptionGeneration, SubscriptionPolicy, SubscriptionScope, SubscriptionState,
};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MembershipEnd {
    pub reason: EndReason,
    pub position: FeedCursor,
    pub at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionMembershipProps {
    pub envelope: RecordEnvelope,
    pub reader: Identity,
    pub scope: SubscriptionScope,
    pub policy: SubscriptionPolicy,
    pub renewed_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub generation: SubscriptionGeneration,
    pub activated_at: FeedCursor,
    pub ended: Option<MembershipEnd>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    try_from = "SubscriptionMembershipProps",
    into = "SubscriptionMembershipProps"
)]
pub struct SubscriptionMembership {
    pub envelope: RecordEnvelope,
    reader: Identity,
    scope: SubscriptionScope,
    policy: SubscriptionPolicy,
    renewed_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    generation: SubscriptionGeneration,
    activated_at: FeedCursor,
    ended: Option<MembershipEnd>,
}

impl SubscriptionMembership {
    pub fn new(props: SubscriptionMembershipProps) -> Result<Self, InvalidSubscriptionField> {
        let invalid = |field, requirement| InvalidSubscriptionField { field, requirement };
        props.policy.validate_reader(&props.reader)?;
        if props.envelope.actor.declared_identity().is_none() {
            return Err(invalid("actor", "membership requires a declared requester"));
        }
        let RecordPosition::Project(position) = props.envelope.position else {
            return Err(invalid("position", "membership belongs to a project feed"));
        };
        if props.envelope.scope.project.is_none() {
            return Err(invalid("project", "membership belongs to a project"));
        }
        if let SubscriptionScope::Project { project_id } = &props.scope
            && props.envelope.scope.project.as_ref() != Some(project_id)
        {
            return Err(invalid("scope", "must belong to the membership project"));
        }
        if props.expires_at <= props.renewed_at {
            return Err(invalid("expiresAt", "must be later than renewedAt"));
        }
        if props
            .ended
            .as_ref()
            .is_some_and(|ended| ended.position.epoch != position.epoch)
        {
            return Err(invalid(
                "ended.position",
                "must belong to the membership epoch",
            ));
        }
        Ok(Self {
            envelope: props.envelope,
            reader: props.reader,
            scope: props.scope,
            policy: props.policy,
            renewed_at: props.renewed_at,
            expires_at: props.expires_at,
            generation: props.generation,
            activated_at: props.activated_at,
            ended: props.ended,
        })
    }
    #[must_use]
    pub fn reader(&self) -> &Identity {
        &self.reader
    }
    #[must_use]
    pub fn scope(&self) -> &SubscriptionScope {
        &self.scope
    }
    #[must_use]
    pub fn policy(&self) -> &SubscriptionPolicy {
        &self.policy
    }
    #[must_use]
    pub fn renewed_at(&self) -> DateTime<Utc> {
        self.renewed_at
    }
    #[must_use]
    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
    #[must_use]
    pub fn generation(&self) -> SubscriptionGeneration {
        self.generation
    }
    #[must_use]
    pub fn activated_at(&self) -> FeedCursor {
        self.activated_at
    }
    #[must_use]
    pub fn ended(&self) -> Option<&MembershipEnd> {
        self.ended.as_ref()
    }

    #[must_use]
    pub fn state_at(
        &self,
        now: DateTime<Utc>,
        delivered_through: Option<FeedCursor>,
        notices_owed: bool,
    ) -> SubscriptionState {
        if let Some(ended) = &self.ended {
            if notices_owed
                && !delivered_through.is_some_and(|through| {
                    through.epoch == ended.position.epoch
                        && through.position >= ended.position.position
                })
            {
                SubscriptionState::Draining
            } else {
                SubscriptionState::Ended {
                    reason: ended.reason,
                }
            }
        } else if now >= self.expires_at {
            SubscriptionState::Ended {
                reason: EndReason::Expired,
            }
        } else {
            SubscriptionState::Active
        }
    }
}
impl TryFrom<SubscriptionMembershipProps> for SubscriptionMembership {
    type Error = InvalidSubscriptionField;
    fn try_from(props: SubscriptionMembershipProps) -> Result<Self, Self::Error> {
        Self::new(props)
    }
}
impl From<SubscriptionMembership> for SubscriptionMembershipProps {
    fn from(record: SubscriptionMembership) -> Self {
        Self {
            envelope: record.envelope,
            reader: record.reader,
            scope: record.scope,
            policy: record.policy,
            renewed_at: record.renewed_at,
            expires_at: record.expires_at,
            generation: record.generation,
            activated_at: record.activated_at,
            ended: record.ended,
        }
    }
}
