//! Map the actual library refusal; writer state is never queried while a write is held.
use super::ProjectAdmissionError;
use fleet_sync_writer::{OpenError, SchemaForm, WriteRefusal};
use message_board::{EpochNumber, RequestResult, SchemaRefusal, TransferId};
use message_board_storage::ProjectStorageError;

pub(super) fn refusal_result(
    error: ProjectAdmissionError,
) -> Result<RequestResult, ProjectAdmissionError> {
    let storage = match &error {
        ProjectAdmissionError::Storage(storage)
        | ProjectAdmissionError::Registry(crate::ProjectRegistryError::Storage(storage)) => {
            Some(storage)
        }
        _ => None,
    };
    if let Some(storage) = storage
        && let Some(result) = gate_result(storage)?
    {
        return Ok(result);
    }
    Err(error)
}

pub(super) fn gate_result(
    error: &ProjectStorageError,
) -> Result<Option<RequestResult>, ProjectAdmissionError> {
    Ok(match error {
        ProjectStorageError::SchemaNotReady(refusal) => {
            Some(RequestResult::SchemaNotReady(refusal.clone()))
        }
        ProjectStorageError::Writer(OpenError::SchemaUnsupported {
            form: SchemaForm::SchemaTooNew { store, supported },
        }) => Some(RequestResult::SchemaNotReady(SchemaRefusal::TooNew {
            store: *store,
            supported: *supported,
        })),
        ProjectStorageError::WriteRefused(refusal) => match refusal {
            WriteRefusal::LeaseExpired { expired_at, .. } => Some(RequestResult::LeaseExpired {
                expired_at: chrono::DateTime::from_timestamp_millis(expired_at.unix_millis())
                    .ok_or_else(|| {
                        ProjectAdmissionError::InvalidRecord(
                            "library lease expiry is outside UTC range".to_owned(),
                        )
                    })?,
            }),
            WriteRefusal::Draining { transfer } => Some(RequestResult::TransferInProgress {
                transfer: TransferId::try_from(transfer.to_string())
                    .map_err(|error| ProjectAdmissionError::InvalidRecord(error.to_string()))?,
            }),
            WriteRefusal::Released | WriteRefusal::NotWriter => {
                Some(RequestResult::NotWriter { current: None })
            }
            WriteRefusal::Superseded { current } => Some(RequestResult::NotWriter {
                current: Some(EpochNumber::try_from(current.value())?),
            }),
            WriteRefusal::Rejected { .. }
            | WriteRefusal::FileInUse
            | WriteRefusal::Closed
            | WriteRefusal::Storage { .. }
            | WriteRefusal::Provisional
            | WriteRefusal::Seeding
            | WriteRefusal::InvalidKey { .. }
            | WriteRefusal::KeyChange { .. }
            | WriteRefusal::StatementRefused { .. } => None,
        },
        _ => None,
    })
}
