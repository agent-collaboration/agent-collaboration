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
use crate::fleet_row_codec::{check_private_id, corrupt};
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
    let rows = crate::generated_b3_legacy::read_legacy_machines(scope)
        .await
        .map_err(StoreError::backend)?;
    convert_all(rows).map(PendingConversion)
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

/// Why one column of one legacy row can't convert. A fixed phrase: the
/// refusal never carries a stored value.
type ColumnRefusal = (&'static str, &'static str);

/// Every legacy row that can't convert, named by its id (or its position in
/// id order when the id itself is invalid) and the offending columns. The
/// owner's switch can be fixed from this alone; it carries no stored values.
fn refusal(refused: &[(String, Vec<ColumnRefusal>)]) -> StoreError {
    let rows = refused
        .iter()
        .map(|(row, columns)| {
            let columns = columns
                .iter()
                .map(|(column, reason)| format!("machine_enrollments.{column} ({reason})"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{row}: {columns}")
        })
        .collect::<Vec<_>>()
        .join("; ");
    corrupt(
        "machine_enrollments",
        format!(
            "B3's switch refuses {} legacy machine row(s): {rows}",
            refused.len()
        ),
    )
}

fn convert_all(rows: Vec<StoredLegacyMachine>) -> Result<Vec<ConvertedMachine>, StoreError> {
    let mut converted = Vec::with_capacity(rows.len());
    let mut refused = Vec::new();
    for (position, row) in rows.into_iter().enumerate() {
        match convert(row) {
            Ok(machine) => converted.push(machine),
            Err((id, columns)) => refused.push((
                id.map_or_else(
                    || format!("row {} in machine_id order", position + 1),
                    |id| format!("machine {id}"),
                ),
                columns,
            )),
        }
    }
    if refused.is_empty() {
        Ok(converted)
    } else {
        Err(refusal(&refused))
    }
}

/// Collects each offending column of one row.
struct CheckedColumns<'row>(&'row mut Vec<ColumnRefusal>);
impl CheckedColumns<'_> {
    fn column<T>(&mut self, column: &'static str, checked: Result<T, &'static str>) -> Option<T> {
        checked.map_err(|reason| self.0.push((column, reason))).ok()
    }
}

fn text(value: Option<String>, class: Option<String>) -> Result<String, &'static str> {
    match (class.as_deref(), value) {
        (Some("text"), Some(value)) => Ok(value),
        (Some("null") | None, _) => Err("is NULL"),
        _ => Err("is not TEXT"),
    }
}

/// Checks every column of one row. On refusal, returns the row's id when the
/// id itself is valid, and each offending column.
fn convert(
    row: StoredLegacyMachine,
) -> Result<ConvertedMachine, (Option<String>, Vec<ColumnRefusal>)> {
    let mut refused = Vec::new();
    let mut check = CheckedColumns(&mut refused);
    let id = check.column(
        "machine_id",
        text(row.machine_id, row.machine_id_class).and_then(|id| {
            check_private_id(&id, "machine_id")
                .map(|()| id)
                .map_err(|_| "is not a canonical UUIDv7")
        }),
    );
    let former_service_id = check.column(
        "service_id",
        text(row.service_id, row.service_id_class).and_then(|id| {
            ServiceId::try_from(id.clone())
                .map(|_| id)
                .map_err(|_| "is not a service id")
        }),
    );
    let label = check.column(
        "machine_label",
        text(row.machine_label, row.machine_label_class).and_then(|label| {
            MachineLabel::try_from(label.clone())
                .map(|_| label)
                .map_err(|_| "is not a machine label")
        }),
    );
    let last_seen_at_ms = check.column(
        "last_seen_at",
        match row.last_seen_at_class.as_deref() {
            Some("null") => Ok(None),
            _ => text(row.last_seen_at, row.last_seen_at_class)
                .and_then(|time| exact_millis(&time))
                .map(Some),
        },
    );
    let created_at_ms = check.column(
        "created_at",
        text(row.created_at, row.created_at_class).and_then(|time| exact_millis(&time)),
    );
    match (id, former_service_id, label, last_seen_at_ms, created_at_ms) {
        (
            Some(id),
            Some(former_service_id),
            Some(label),
            Some(last_seen_at_ms),
            Some(created_at_ms),
        ) => Ok(ConvertedMachine {
            id,
            former_service_id,
            label,
            last_seen_at_ms,
            created_at_ms,
        }),
        (id, ..) => Err((id, refused)),
    }
}

/// An RFC 3339 time, with any offset, as exact Unix milliseconds. Today's
/// writer stores millisecond times, so a finer fraction or an unparseable
/// value is refused, never rounded.
fn exact_millis(value: &str) -> Result<i64, &'static str> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|_| "is not an RFC 3339 time")?
        .with_timezone(&Utc);
    let millis = parsed.timestamp_millis();
    let exact =
        DateTime::<Utc>::from_timestamp_millis(millis).ok_or("is outside the millisecond range")?;
    if exact != parsed {
        return Err("is finer than a millisecond");
    }
    Ok(millis)
}

#[cfg(test)]
#[path = "b3_registry_conversion_test.rs"]
mod tests;
