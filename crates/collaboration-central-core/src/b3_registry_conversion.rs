//! B3's switch for today's machine rows (central-registry v3, b3-sql rev .4).
//! Inside the migration's transaction, around the v3 SQL that creates
//! `machine_installations` and drops today's `machine_enrollments`:
//!
//! - **before**: refuse while any legacy revocation or readmission is
//!   unfinished in the journal, then read and convert every row in Rust: the
//!   peer id stays the machine's id, the routing id becomes its former id, and
//!   each RFC 3339 time becomes exact milliseconds. A null last-seen stays
//!   null. An invalid or sub-millisecond time fails, and nothing commits;
//! - **after**: insert the converted rows.
//!
//! The v3 corpus is staged and unregistered until the B3 reader slice.
use crate::fleet_row_codec::{check_private_id, corrupt, required};
use crate::generated_b3_legacy::StoredLegacyMachine;
use crate::machine_enrollment_record::{READMISSION_TASK_KIND, REVOKE_TASK_KIND};
use chrono::{DateTime, Utc};
use durable_steps::{JournalScope, StoreError, storage::TransactionScope};
use message_board::{MachineLabel, ServiceId};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

/// One of today's machines, converted and waiting for its new table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConvertedMachine {
    pub id: String,
    pub former_service_id: String,
    pub label: String,
    pub last_seen_at_ms: Option<i64>,
    pub created_at_ms: i64,
}

/// Today's rows, converted, and the precondition passed.
pub(crate) struct PendingConversion(Vec<ConvertedMachine>);

pub(crate) async fn prepare<S>(scope: &mut S) -> Result<PendingConversion, StoreError>
where
    S: TransactionScope + JournalScope,
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> Option<String>: Type<S::Database> + Decode<'value, S::Database>,
    for<'row> &'row str: ColumnIndex<<S::Database as Database>::Row>,
{
    for kind in [REVOKE_TASK_KIND, READMISSION_TASK_KIND] {
        if !scope.running_request_keys(kind).await?.is_empty() {
            return Err(StoreError::Aborted {
                reason: format!(
                    "B3's switch needs every legacy {kind} operation settled first; one is unfinished"
                ),
            });
        }
    }
    crate::generated_b3_legacy::read_legacy_machines(scope)
        .await
        .map_err(StoreError::backend)?
        .into_iter()
        .map(convert)
        .collect::<Result<Vec<_>, _>>()
        .map(PendingConversion)
}

pub(crate) async fn apply<S>(
    scope: &mut S,
    pending: PendingConversion,
    now_ms: i64,
) -> Result<usize, StoreError>
where
    S: TransactionScope,
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> String: Type<S::Database> + Encode<'value, S::Database>,
    for<'value> Option<String>: Type<S::Database> + Encode<'value, S::Database>,
{
    let count = pending.0.len();
    for machine in pending.0 {
        crate::generated_b3_registry::insert_converted_installation(
            scope,
            machine.id,
            machine.former_service_id,
            machine.label,
            machine.last_seen_at_ms.map(|at| at.to_string()),
            machine.created_at_ms.to_string(),
            now_ms.to_string(),
        )
        .await
        .map_err(StoreError::backend)?;
    }
    Ok(count)
}

const FIELD: &str = "machine_enrollments";

fn text(value: Option<String>, class: Option<String>, field: &str) -> Result<String, StoreError> {
    if required(class, field)? != "text" {
        return Err(corrupt(field, "column is not TEXT"));
    }
    required(value, field)
}

fn convert(row: StoredLegacyMachine) -> Result<ConvertedMachine, StoreError> {
    let id = text(
        row.machine_id,
        row.machine_id_class,
        "machine_enrollments.machine_id",
    )?;
    check_private_id(&id, "machine_enrollments.machine_id")?;
    let former_service_id = text(
        row.service_id,
        row.service_id_class,
        "machine_enrollments.service_id",
    )?;
    ServiceId::try_from(former_service_id.clone())
        .map_err(|error| corrupt("machine_enrollments.service_id", error))?;
    let label = text(
        row.machine_label,
        row.machine_label_class,
        "machine_enrollments.machine_label",
    )?;
    MachineLabel::try_from(label.clone())
        .map_err(|error| corrupt("machine_enrollments.machine_label", error))?;
    let last_seen_at_ms =
        match required(row.last_seen_at_class, "machine_enrollments.last_seen_at")?.as_str() {
            "null" => None,
            "text" => Some(exact_millis(
                &required(row.last_seen_at, "machine_enrollments.last_seen_at")?,
                "machine_enrollments.last_seen_at",
            )?),
            _ => return Err(corrupt(FIELD, "last_seen_at is neither TEXT nor NULL")),
        };
    let created_at_ms = exact_millis(
        &text(
            row.created_at,
            row.created_at_class,
            "machine_enrollments.created_at",
        )?,
        "machine_enrollments.created_at",
    )?;
    Ok(ConvertedMachine {
        id,
        former_service_id,
        label,
        last_seen_at_ms,
        created_at_ms,
    })
}

/// An RFC 3339 time, with any offset, as exact Unix milliseconds. Today's
/// writer stores millisecond times, so a finer fraction or an unparseable
/// value is a corrupt row, never rounded.
fn exact_millis(value: &str, field: &str) -> Result<i64, StoreError> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|error| corrupt(field, error))?
        .with_timezone(&Utc);
    let millis = parsed.timestamp_millis();
    let exact = DateTime::<Utc>::from_timestamp_millis(millis)
        .ok_or_else(|| corrupt(field, "time outside the millisecond range"))?;
    if exact != parsed {
        return Err(corrupt(field, "time is finer than a millisecond"));
    }
    Ok(millis)
}

#[cfg(test)]
#[path = "b3_registry_conversion_test.rs"]
mod tests;
