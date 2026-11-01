//! Invalid domain data is permanent; storage failures must roll the pass back.
use collaboration_protocol::{
    AutomationAnswer, AutomationRefusal, AutomationRequestConstraint, AutomationValidationError,
    ObservationTimestamp, limits::LimitExceeded,
};
use durable_steps::{StoreError, Uuid};

#[derive(Debug, thiserror::Error)]
pub(crate) enum AutomationStoreError {
    #[error("automation refused: {0:?}")]
    Refused(AutomationRefusal),
    #[error(transparent)]
    Storage(#[from] StoreError),
    #[error(transparent)]
    Validation(#[from] AutomationValidationError),
    #[error("invalid automation record field {0}")]
    InvalidRecord(&'static str),
    #[error("invalid automation request field {field}: {reason:?}")]
    InvalidRequest {
        field: &'static str,
        reason: AutomationRequestConstraint,
    },
}

impl From<sqlx_core::Error> for AutomationStoreError {
    fn from(error: sqlx_core::Error) -> Self {
        Self::Storage(StoreError::backend(error))
    }
}

impl From<LimitExceeded> for AutomationStoreError {
    fn from(error: LimitExceeded) -> Self {
        Self::Validation(error.into())
    }
}

impl AutomationStoreError {
    pub fn into_answer(self) -> Result<AutomationAnswer, StoreError> {
        let refusal = match self {
            Self::Refused(refusal) => refusal,
            Self::Storage(error) => return Err(error),
            Self::Validation(error) => AutomationRefusal::Validation { error },
            Self::InvalidRecord(field) => AutomationRefusal::InvalidRecord {
                field: field.into(),
            },
            Self::InvalidRequest { field, reason } => AutomationRefusal::InvalidRequest {
                field: field.into(),
                reason,
            },
        };
        Ok(AutomationAnswer::Refused(refusal))
    }
}

pub(crate) type AutomationStoreResult<T> = Result<T, AutomationStoreError>;

pub(crate) fn required(
    value: Option<String>,
    field: &'static str,
) -> AutomationStoreResult<String> {
    value.ok_or(AutomationStoreError::InvalidRecord(field))
}

pub(crate) fn timestamp_millis(
    value: Option<String>,
    field: &'static str,
) -> AutomationStoreResult<i64> {
    let millis = required(value, field)?
        .parse()
        .map_err(|_| AutomationStoreError::InvalidRecord(field))?;
    chrono::DateTime::from_timestamp_millis(millis)
        .ok_or(AutomationStoreError::InvalidRecord(field))?;
    Ok(millis)
}

pub(crate) fn timestamp(millis: i64) -> AutomationStoreResult<ObservationTimestamp> {
    let time = chrono::DateTime::from_timestamp_millis(millis)
        .ok_or(AutomationStoreError::InvalidRecord("timestamp"))?;
    time.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("timestamp"))
}

pub(crate) fn row_id(value: &str, field: &'static str) -> AutomationStoreResult<Uuid> {
    let id = Uuid::parse_str(value).map_err(|_| AutomationStoreError::InvalidRecord(field))?;
    if !runtime_primitives::uuid_v7(id) || id.to_string() != value {
        return Err(AutomationStoreError::InvalidRecord(field));
    }
    Ok(id)
}
