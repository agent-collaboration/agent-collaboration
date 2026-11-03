//! Typed claim persistence within the caller-owned Fleet transaction.
use crate::{
    claim_reservation_contract::{ClaimReservationOutcome, ClaimReservationState},
    fleet_row_codec::{check_millis, check_private_id, corrupt, required},
    generated_fleet_lease as queries,
    lease_projection::LeaseProjectionResult,
};
use durable_steps::{CallId, StoreError, storage::TransactionScope};
use fleet_sync_protocol::{DatabaseId, TransferId};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) struct ClaimReceiptInput {
    pub id: durable_steps::Uuid,
    pub call: CallId,
    pub attempt: TransferId,
    pub database: DatabaseId,
    pub result: LeaseProjectionResult,
    pub now: chrono::DateTime<chrono::Utc>,
}

pub(crate) async fn read_receipt<S: TransactionScope>(
    scope: &mut S,
    call: &CallId,
) -> Result<Option<ClaimReceipt>, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> String: Type<S::Database> + Encode<'value, S::Database>,
    for<'value> Option<String>: Type<S::Database> + Decode<'value, S::Database>,
    for<'row> &'row str: ColumnIndex<<S::Database as Database>::Row>,
{
    queries::read_claim_settlement_receipt(
        scope,
        call.operation.as_uuid().to_string(),
        call.step.as_str().into(),
    )
    .await
    .map_err(StoreError::backend)?
    .map(|row| decode_receipt(row, call))
    .transpose()
}

pub(crate) async fn insert_receipt<S: TransactionScope>(
    scope: &mut S,
    receipt: ClaimReceiptInput,
) -> Result<(), StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> String: Type<S::Database> + Encode<'value, S::Database>,
    for<'value> Option<String>: Type<S::Database> + Encode<'value, S::Database>,
{
    check_private_id(&receipt.id.to_string(), "claim.receipt_id")?;
    let columns = result_columns(&receipt.result);
    queries::insert_claim_settlement_receipt(
        scope,
        receipt.id.to_string(),
        receipt.call.operation.as_uuid().to_string(),
        receipt.call.step.as_str().into(),
        receipt.attempt.as_uuid().to_string(),
        receipt.database.as_uuid().to_string(),
        columns.kind.into(),
        columns.epoch,
        columns.incarnation,
        columns.change,
        receipt.now.timestamp_millis().to_string(),
    )
    .await
    .map_err(StoreError::backend)
}

pub(crate) async fn read_reservation<Scope: TransactionScope>(
    scope: &mut Scope,
    database: DatabaseId,
    attempt: TransferId,
) -> Result<Option<ClaimReservationState>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    queries::read_claim_reservation(scope, attempt.as_uuid().to_string())
        .await
        .map_err(StoreError::backend)?
        .map(|stored| {
            let stored = decode_reservation(stored)?;
            if stored.database != database || stored.attempt != attempt {
                return Err(corrupt(
                    "claim.project_id",
                    "attempt belongs to another project",
                ));
            }
            Ok(stored.state)
        })
        .transpose()
}

pub(crate) async fn write_reservation<Scope: TransactionScope>(
    scope: &mut Scope,
    database: DatabaseId,
    attempt: TransferId,
    state: ClaimReservationState,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    let (kind, outcome) = match state {
        ClaimReservationState::Active => ("active", None),
        ClaimReservationState::Settled {
            outcome: ClaimReservationOutcome::Granted,
        } => ("settled", Some("granted".into())),
        ClaimReservationState::Settled {
            outcome: ClaimReservationOutcome::Refused,
        } => ("settled", Some("refused".into())),
    };
    queries::store_claim_reservation(
        scope,
        attempt.as_uuid().to_string(),
        database.as_uuid().to_string(),
        kind.into(),
        outcome,
        now.timestamp_millis().to_string(),
    )
    .await
    .map_err(StoreError::backend)?;
    Ok(())
}

pub(crate) struct ClaimReservationRecord {
    pub attempt: TransferId,
    pub database: DatabaseId,
    pub state: ClaimReservationState,
}

pub(crate) fn decode_reservation(
    row: queries::StoredClaimReservation,
) -> Result<ClaimReservationRecord, StoreError> {
    let attempt = required(row.attempt_id, "claim.attempt_id")?;
    let project = required(row.project_id, "claim.project_id")?;
    check_private_id(&attempt, "claim.attempt_id")?;
    check_private_id(&project, "claim.project_id")?;
    check_millis(row.created_at, row.created_class, "claim.created_at")?;
    check_millis(row.updated_at, row.updated_class, "claim.updated_at")?;
    let kind = required(row.state_kind, "claim.state_kind")?;
    let state = match (kind.as_str(), row.outcome.as_deref()) {
        ("active", None) => ClaimReservationState::Active,
        ("settled", Some("granted")) => ClaimReservationState::Settled {
            outcome: ClaimReservationOutcome::Granted,
        },
        ("settled", Some("refused")) => ClaimReservationState::Settled {
            outcome: ClaimReservationOutcome::Refused,
        },
        _ => {
            return Err(corrupt(
                "claim.state",
                "unknown state or incoherent settlement outcome",
            ));
        }
    };
    Ok(ClaimReservationRecord {
        attempt: TransferId::new(
            durable_steps::Uuid::parse_str(&attempt).map_err(StoreError::backend)?,
        )
        .map_err(StoreError::backend)?,
        database: DatabaseId::try_from(project).map_err(StoreError::backend)?,
        state,
    })
}

struct ReceiptColumns {
    kind: &'static str,
    epoch: Option<String>,
    incarnation: Option<String>,
    change: Option<String>,
}
fn result_columns(result: &LeaseProjectionResult) -> ReceiptColumns {
    let (kind, stored) = match result {
        LeaseProjectionResult::Applied => ("applied", None),
        LeaseProjectionResult::Duplicate => ("duplicate", None),
        LeaseProjectionResult::Older { stored } => ("older", Some(*stored)),
        LeaseProjectionResult::Conflict { stored } => ("conflict", Some(*stored)),
    };
    let (epoch, incarnation, change) = match stored {
        None => (None, None, None),
        Some(version) => {
            let (epoch, incarnation, change) = version.components();
            (
                Some(epoch.value().to_string()),
                Some(incarnation.value().to_string()),
                Some(change.get().to_string()),
            )
        }
    };
    ReceiptColumns {
        kind,
        epoch,
        incarnation,
        change,
    }
}
pub(crate) struct ClaimReceipt {
    pub attempt: fleet_sync_protocol::TransferId,
    pub database: fleet_sync_protocol::DatabaseId,
    pub result: LeaseProjectionResult,
}
fn decode_receipt(
    row: queries::StoredClaimSettlementReceipt,
    call: &CallId,
) -> Result<ClaimReceipt, StoreError> {
    use crate::fleet_row_codec::{check_millis, check_private_id};
    let receipt_id = required(row.receipt_id, "claim.receipt_id")?;
    check_private_id(&receipt_id, "claim.receipt_id")?;
    let operation = required(row.operation_id, "claim.operation_id")?;
    let step = required(row.step_name, "claim.step_name")?;
    check_private_id(&operation, "claim.operation_id")?;
    if operation != call.operation.as_uuid().to_string() || step != call.step.as_str() {
        return Err(corrupt("claim.receipt", "receipt belongs to another call"));
    }
    let attempt = required(row.attempt_id, "claim.attempt_id")?;
    let database = required(row.project_id, "claim.project_id")?;
    check_private_id(&attempt, "claim.attempt_id")?;
    check_private_id(&database, "claim.project_id")?;
    check_millis(
        row.created_at,
        row.created_class,
        "claim.receipt.created_at",
    )?;
    check_millis(
        row.updated_at,
        row.updated_class,
        "claim.receipt.updated_at",
    )?;
    let kind = required(row.result_kind, "claim.result_kind")?;
    let result = match (
        kind.as_str(),
        row.stored_epoch,
        row.stored_incarnation,
        row.stored_change,
    ) {
        ("applied", None, None, None) => LeaseProjectionResult::Applied,
        ("duplicate", None, None, None) => LeaseProjectionResult::Duplicate,
        ("older" | "conflict", Some(epoch), Some(incarnation), Some(change)) => {
            let epoch = fleet_sync_protocol::Epoch::new(decode_version_counter(
                epoch,
                "claim.stored_epoch",
            )?)
            .map_err(StoreError::backend)?;
            let incarnation = crate::RegistryIncarnation::new(decode_version_counter(
                incarnation,
                "claim.stored_incarnation",
            )?)
            .map_err(StoreError::backend)?;
            let change =
                std::num::NonZeroU64::new(decode_version_counter(change, "claim.stored_change")?)
                    .ok_or_else(|| corrupt("claim.stored_change", "change must be positive"))?;
            let stored = crate::lease_projection::LeaseProjectionVersion::from_components(
                epoch,
                incarnation,
                change,
            );
            if kind == "older" {
                LeaseProjectionResult::Older { stored }
            } else {
                LeaseProjectionResult::Conflict { stored }
            }
        }
        _ => {
            return Err(corrupt(
                "claim.result",
                "unknown result or incoherent stored version",
            ));
        }
    };
    Ok(ClaimReceipt {
        attempt: fleet_sync_protocol::TransferId::new(
            durable_steps::Uuid::parse_str(&attempt).map_err(StoreError::backend)?,
        )
        .map_err(StoreError::backend)?,
        database: fleet_sync_protocol::DatabaseId::try_from(database)
            .map_err(StoreError::backend)?,
        result,
    })
}
fn decode_version_counter(value: String, field: &str) -> Result<u64, StoreError> {
    let number = value
        .parse::<u64>()
        .map_err(|error| corrupt(field, error))?;
    if number.to_string() != value {
        return Err(corrupt(field, "counter must be canonical decimal u64"));
    }
    Ok(number)
}
