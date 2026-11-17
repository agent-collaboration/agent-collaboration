use crate::{AlarmDecisionError, EngineTime, next_five_minute_mark};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetirementThreshold {
    After(NonZeroU32),
    AfterContinuousFailure(chrono::Duration),
}

impl RetirementThreshold {
    pub const DEFAULT: Self = Self::After(NonZeroU32::MIN.saturating_add(1));
    pub const NOTICE_UNITS: Self = Self::AfterContinuousFailure(chrono::Duration::days(60));
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemFailureClass {
    CorruptRow {
        column: String,
        raw_value: Option<String>,
    },
    ConstraintViolated {
        constraint: central_storage::ConstraintKind,
    },
    StatementRejected,
    Refused {
        refusal: String,
    },
    OtherStoreUnavailable,
}

impl ItemFailureClass {
    pub const fn storage_tag(&self) -> &'static str {
        match self {
            Self::CorruptRow { .. } => "corrupt_row",
            Self::ConstraintViolated { .. } => "constraint_violated",
            Self::StatementRejected => "statement_rejected",
            Self::Refused { .. } => "refused",
            Self::OtherStoreUnavailable => "other_store_unavailable",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AttemptState {
    Failing { next_try_at: EngineTime },
    Retired { retired_at: EngineTime },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailureRecord {
    pub attempt_id: uuid::Uuid,
    pub failure_count: NonZeroU32,
    pub state: AttemptState,
    pub last_failure: ItemFailureClass,
    pub first_failed_at: EngineTime,
    pub last_failed_at: EngineTime,
    pub created_at: EngineTime,
    pub updated_at: EngineTime,
}

/// What one observed failure does to a subject's attempt record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FailureTransition {
    /// Write this record in the failure's batch.
    Record(FailureRecord),
    /// The subject is retired, or failing with its next try still ahead: the
    /// run that saw this failure should not have worked it yet, so the failure
    /// counts for nothing and nothing is written (R4: each further failure
    /// retries on the next mark).
    Unchanged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum FailureUpdateError {
    #[error("the failure count is exhausted")]
    CountExhausted,
    #[error("the supplied attempt identifier is not UUIDv7")]
    InvalidAttemptId,
    #[error(transparent)]
    Alarm(#[from] AlarmDecisionError),
}

/// The caller supplies the new row ID; an existing row retains its identity.
pub fn after_failure(
    prior: Option<&FailureRecord>,
    failure: ItemFailureClass,
    now: EngineTime,
    threshold: RetirementThreshold,
    new_attempt_id: uuid::Uuid,
) -> Result<FailureTransition, FailureUpdateError> {
    if !crate::supported_time(now) {
        return Err(FailureUpdateError::Alarm(AlarmDecisionError::OutOfRange));
    }
    match prior.map(|record| &record.state) {
        Some(AttemptState::Retired { .. }) => return Ok(FailureTransition::Unchanged),
        Some(AttemptState::Failing { next_try_at }) if *next_try_at > now => {
            return Ok(FailureTransition::Unchanged);
        }
        Some(AttemptState::Failing { .. }) | None => {}
    }
    let count = prior
        .map_or(Some(1), |record| record.failure_count.get().checked_add(1))
        .and_then(NonZeroU32::new)
        .ok_or(FailureUpdateError::CountExhausted)?;
    let attempt_id = prior.map_or(new_attempt_id, |record| record.attempt_id);
    if attempt_id.get_version_num() != 7 {
        return Err(FailureUpdateError::InvalidAttemptId);
    }
    let first_failed_at = prior.map_or(now, |record| record.first_failed_at);
    let retire = match threshold {
        RetirementThreshold::After(limit) => count >= limit,
        RetirementThreshold::AfterContinuousFailure(duration) => {
            now.signed_duration_since(first_failed_at) >= duration
        }
    };
    let state = if retire {
        AttemptState::Retired { retired_at: now }
    } else {
        AttemptState::Failing {
            next_try_at: next_five_minute_mark(now)?,
        }
    };
    Ok(FailureTransition::Record(FailureRecord {
        attempt_id,
        failure_count: count,
        state,
        last_failure: failure,
        first_failed_at,
        last_failed_at: now,
        created_at: prior.map_or(now, |record| record.created_at),
        updated_at: now,
    }))
}

#[cfg(test)]
#[path = "attempts_test.rs"]
mod tests;
