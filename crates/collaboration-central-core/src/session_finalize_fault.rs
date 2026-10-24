//! Conformance fault data crosses the checked callback boundary without a host.

use durable_steps::{StoreError, storage::TransactionScope};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::num::NonZeroU64;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionFinalizeFault {
    Callback,
    Sql,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum SessionFinalizeFailure {
    Callback {
        written: NonZeroU64,
    },
    Sql {
        written: NonZeroU64,
        cause: StoreError,
    },
}

// Keep the real outer StoreError class and retry semantics. Typed diagnostic
// data travels in that error's existing text payload; it is not a new refusal.
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "centralConformance",
    content = "failure",
    rename_all = "camelCase",
    deny_unknown_fields
)]
enum RollbackObservation {
    SessionFinalize(SessionFinalizeFailure),
}

impl SessionFinalizeFailure {
    pub fn fault(&self) -> SessionFinalizeFault {
        match self {
            Self::Callback { .. } => SessionFinalizeFault::Callback,
            Self::Sql { .. } => SessionFinalizeFault::Sql,
        }
    }
    pub(crate) fn from_store_error(error: &StoreError) -> Option<Self> {
        let text = match error {
            StoreError::Aborted { reason } => reason,
            StoreError::Backend { message } => message,
            _ => return None,
        };
        let RollbackObservation::SessionFinalize(observation) = serde_json::from_str(text).ok()?;
        match (&observation, error) {
            (Self::Callback { .. }, StoreError::Aborted { .. }) => Some(observation),
            (
                Self::Sql {
                    cause: StoreError::Backend { .. },
                    ..
                },
                StoreError::Backend { .. },
            ) => Some(observation),
            _ => None,
        }
    }
    fn store_error(self) -> StoreError {
        let outer_callback = matches!(&self, Self::Callback { .. });
        match serde_json::to_string(&RollbackObservation::SessionFinalize(self)) {
            Ok(detail) if outer_callback => StoreError::Aborted { reason: detail },
            Ok(detail) => StoreError::Backend { message: detail },
            Err(error) => StoreError::backend(error),
        }
    }
}

pub(crate) async fn changes<Scope: TransactionScope>(scope: &mut Scope) -> Result<u64, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let value = crate::generated_session_conformance::read_session_conformance_changes(scope)
        .await
        .map_err(StoreError::backend)?
        .and_then(|row| row.total_changes)
        .ok_or_else(|| StoreError::backend("session write counter missing"))?;
    value.parse().map_err(StoreError::backend)
}

pub(crate) async fn fail_after_writes<Scope: TransactionScope>(
    scope: &mut Scope,
    fault: SessionFinalizeFault,
    before: u64,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let written = changes(scope)
        .await?
        .checked_sub(before)
        .and_then(NonZeroU64::new)
        .ok_or_else(|| StoreError::backend("session finalize fault reached no callback writes"))?;
    let failure = match fault {
        SessionFinalizeFault::Callback => SessionFinalizeFailure::Callback { written },
        SessionFinalizeFault::Sql => {
            match crate::generated_session_conformance::session_conformance_sql_failure(
                scope,
                i64::MIN.to_string(),
            )
            .await
            {
                Err(error) => SessionFinalizeFailure::Sql {
                    written,
                    cause: StoreError::backend(error),
                },
                Ok(row) => {
                    let _unexpected_value = row.and_then(|row| row.value);
                    return Err(StoreError::Aborted {
                        reason: "session SQL fault did not raise an SQLx error".into(),
                    });
                }
            }
        }
    };
    Err(failure.store_error())
}
