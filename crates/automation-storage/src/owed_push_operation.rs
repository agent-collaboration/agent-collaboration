//! Closed automation-owned operations whose push effects belong to interaction storage.
use crate::StorageError;
use chrono::{DateTime, Utc};
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, PushDeliveryState, PushId, PushKind, PushOrigin,
    PushRecordDraft, RouterOriginRef,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PushOperationId(String);
impl PushOperationId {
    pub fn generate() -> Self {
        Self(
            agent_automation::OperationId::generate()
                .as_str()
                .to_owned(),
        )
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for PushOperationId {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        agent_automation::OperationId::try_from(value.clone())
            .map_err(|_| "push operation ID must be a canonical UUIDv7")?;
        Ok(Self(value))
    }
}
impl From<PushOperationId> for String {
    fn from(value: PushOperationId) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PushOperation {
    Admit {
        push: Box<PushRecordDraft>,
    },
    SettleScheduleRun {
        push_id: PushId,
        origin_ref: RouterOriginRef,
        state: PushDeliveryState,
        receipt: Box<DeliveryReceipt>,
        settled_at: DateTime<Utc>,
    },
}
impl PushOperation {
    pub fn push_id(&self) -> &PushId {
        match self {
            Self::Admit { push } => &push.push_id,
            Self::SettleScheduleRun { push_id, .. } => push_id,
        }
    }
    pub(crate) fn validate(&self) -> Result<(), StorageError> {
        match self {
            Self::Admit { push } => {
                let pending = push
                    .clone()
                    .into_pending()
                    .map_err(|_| StorageError::InvalidRecord)?;
                if pending.kind != PushKind::Wake
                    || pending.origin != PushOrigin::Router(PushKind::Wake)
                {
                    return Err(StorageError::InvalidRecord);
                }
                let encoded = pending
                    .origin_router_ref
                    .as_deref()
                    .ok_or(StorageError::InvalidRecord)?;
                let origin = RouterOriginRef::parse_canonical(encoded)
                    .map_err(|_| StorageError::InvalidRecord)?;
                if origin.supports_kind(PushKind::Wake) {
                    Ok(())
                } else {
                    Err(StorageError::InvalidRecord)
                }
            }
            Self::SettleScheduleRun {
                origin_ref,
                state,
                receipt,
                ..
            } => {
                if !matches!(origin_ref, RouterOriginRef::ScheduleRun { .. }) {
                    return Err(StorageError::InvalidRecord);
                }
                origin_ref
                    .canonical_string()
                    .map_err(|_| StorageError::InvalidRecord)?;
                let matching = match state {
                    PushDeliveryState::Delivered => matches!(
                        receipt.outcome,
                        DeliveryOutcome::Started
                            | DeliveryOutcome::Steered
                            | DeliveryOutcome::StartedOrSteered
                            | DeliveryOutcome::Queued
                            | DeliveryOutcome::PeerMessageWritten
                    ),
                    PushDeliveryState::OutcomeUnknown => {
                        receipt.outcome == DeliveryOutcome::Unknown
                    }
                    PushDeliveryState::Pending => {
                        matches!(receipt.outcome, DeliveryOutcome::Rejected(_))
                    }
                    PushDeliveryState::Attempted
                    | PushDeliveryState::Held
                    | PushDeliveryState::Rejected => false,
                };
                if matching {
                    Ok(())
                } else {
                    Err(StorageError::InvalidRecord)
                }
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwedPushOperation {
    pub operation_id: PushOperationId,
    pub push_id: PushId,
    pub operation: PushOperation,
    pub owed_at: DateTime<Utc>,
}
impl OwedPushOperation {
    pub(crate) fn validate(&self) -> Result<(), StorageError> {
        if self.operation.push_id() != &self.push_id {
            return Err(StorageError::InvalidRecord);
        }
        self.operation.validate()
    }
    pub(crate) fn same_input(&self, other: &Self) -> Result<bool, StorageError> {
        Ok(
            serde_json::to_value(self).map_err(|_| StorageError::InvalidRecord)?
                == serde_json::to_value(other).map_err(|_| StorageError::InvalidRecord)?,
        )
    }
}
